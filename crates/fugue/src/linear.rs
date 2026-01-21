use std::collections::BTreeSet;

use anyhow::{anyhow, Context as _};
use fugue_core::issue::{
    compute_ready_issues, CreateParams, Issue, ListFilter, Status, UpdateParams,
};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LinearBackend {
    client: reqwest::Client,
    graphql_url: String,
    api_key: String,
    team_id: String,
    project_id: Option<String>,
    #[allow(dead_code)]
    allowed_authors: Vec<String>,
}

impl LinearBackend {
    pub fn new(
        team_id: String,
        project_id: Option<String>,
        api_key: String,
        allowed_authors: Vec<String>,
        graphql_url: String,
    ) -> anyhow::Result<Self> {
        if api_key.trim().is_empty() {
            return Err(anyhow!("missing linear api key"));
        }
        if team_id.trim().is_empty() {
            return Err(anyhow!("missing linear team id"));
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/json"),
        );

        let client = reqwest::Client::builder()
            .user_agent(format!("fugue/{}", env!("CARGO_PKG_VERSION")))
            .default_headers(headers)
            .build()
            .context("build reqwest client")?;

        Ok(Self {
            client,
            graphql_url,
            api_key,
            team_id,
            project_id: project_id.filter(|s| !s.trim().is_empty()),
            allowed_authors,
        })
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Issue> {
        let query = r#"
            query Issue($id: String!) {
                issue(id: $id) {
                    identifier
                    title
                    description
                    priority
                    createdAt
                    state { type }
                    labels { nodes { name } }
                    parent { identifier }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            issue: LinearIssue,
        }

        let data: Data = self
            .graphql(query, Some(serde_json::json!({ "id": id })))
            .await
            .context("linear get issue")?;

        Ok(to_issue(&data.issue))
    }

    pub async fn list(&self, filter: ListFilter) -> anyhow::Result<Vec<Issue>> {
        let mut filter_obj = serde_json::Map::new();
        if let Some(project_id) = self.project_id.as_ref() {
            filter_obj.insert(
                "project".to_owned(),
                serde_json::json!({ "id": { "eq": project_id } }),
            );
        } else {
            filter_obj.insert(
                "team".to_owned(),
                serde_json::json!({ "id": { "eq": self.team_id } }),
            );
        }

        if !filter.status.is_empty() {
            let mut types = Vec::new();
            for s in &filter.status {
                match s {
                    Status::Open => {
                        types.extend(["backlog", "unstarted", "started"]);
                    }
                    Status::Closed => {
                        types.extend(["completed", "canceled"]);
                    }
                    Status::Blocked => {
                        types.extend(["backlog", "unstarted", "started"]);
                    }
                }
            }
            let types = {
                let set: BTreeSet<&str> = types.into_iter().collect();
                set.into_iter().collect::<Vec<_>>()
            };
            if !types.is_empty() {
                filter_obj.insert(
                    "state".to_owned(),
                    serde_json::json!({ "type": { "in": types } }),
                );
            }
        }

        let query = r#"
            query Issues($filter: IssueFilter) {
                issues(filter: $filter, first: 100) {
                    nodes {
                        identifier
                        title
                        description
                        priority
                        createdAt
                        state { type }
                        labels { nodes { name } }
                        parent { identifier }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            issues: IssueConn,
        }
        #[derive(Debug, Deserialize)]
        struct IssueConn {
            nodes: Vec<LinearIssue>,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "filter": serde_json::Value::Object(filter_obj),
                })),
            )
            .await
            .context("linear list issues")?;

        let mut issues = Vec::new();
        for li in data.issues.nodes {
            let iss = to_issue(&li);

            if !filter.labels.is_empty() {
                let label_set: BTreeSet<&str> = iss.labels.iter().map(String::as_str).collect();
                if !filter.labels.iter().all(|l| label_set.contains(l.as_str())) {
                    continue;
                }
            }

            issues.push(iss);
        }

        Ok(issues)
    }

    pub async fn ready(&self) -> anyhow::Result<Vec<Issue>> {
        let open = self
            .list(ListFilter {
                status: vec![Status::Open],
                labels: vec![],
            })
            .await?;

        let ready = compute_ready_issues(open)
            .into_iter()
            .filter(|i| i.status != Status::Blocked)
            .collect();

        Ok(ready)
    }

    pub async fn create(&self, params: CreateParams) -> anyhow::Result<Issue> {
        if !params.links.is_empty() {
            return Err(anyhow!("linear backend does not support `links`"));
        }

        let linear_priority = map_priority_to_linear(params.priority);

        let mut label_ids = Vec::new();
        let issue_type = params.issue_type.trim().to_owned();
        if !issue_type.is_empty() {
            if let Ok(Some(id)) = self.find_label_id(&format!("type:{issue_type}")).await {
                label_ids.push(id);
            }
        }
        for name in &params.labels {
            if let Ok(Some(id)) = self.find_label_id(name).await {
                label_ids.push(id);
            }
        }

        let mut input = serde_json::Map::new();
        input.insert("title".to_owned(), serde_json::Value::String(params.title));
        input.insert(
            "teamId".to_owned(),
            serde_json::Value::String(self.team_id.clone()),
        );
        input.insert(
            "priority".to_owned(),
            serde_json::Value::Number(linear_priority.into()),
        );
        if let Some(project_id) = self.project_id.as_ref() {
            input.insert(
                "projectId".to_owned(),
                serde_json::Value::String(project_id.clone()),
            );
        }
        if !params.description.trim().is_empty() {
            input.insert(
                "description".to_owned(),
                serde_json::Value::String(params.description),
            );
        }
        if !label_ids.is_empty() {
            input.insert(
                "labelIds".to_owned(),
                serde_json::Value::Array(
                    label_ids
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }

        if let Some(parent) = params.dependencies.first() {
            match self.resolve_issue_id(parent).await {
                Ok(id) => {
                    input.insert("parentId".to_owned(), serde_json::Value::String(id));
                }
                Err(err) => {
                    tracing::warn!(parent = %parent, error = %err, "failed to resolve linear parent issue");
                }
            }
        }

        let query = r#"
            mutation IssueCreate($input: IssueCreateInput!) {
                issueCreate(input: $input) {
                    success
                    issue {
                        identifier
                        title
                        description
                        priority
                        createdAt
                        state { type }
                        labels { nodes { name } }
                        parent { identifier }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "issueCreate")]
            issue_create: IssueCreate,
        }
        #[derive(Debug, Deserialize)]
        struct IssueCreate {
            success: bool,
            issue: LinearIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({ "input": serde_json::Value::Object(input) })),
            )
            .await
            .context("linear create issue")?;

        if !data.issue_create.success {
            return Err(anyhow!("linear issue creation failed"));
        }

        Ok(to_issue(&data.issue_create.issue))
    }

    pub async fn update(&self, id: &str, params: UpdateParams) -> anyhow::Result<Issue> {
        if params.links.is_some() {
            return Err(anyhow!("linear backend does not support `links`"));
        }

        let issue_id = self.resolve_issue_id(id).await?;

        let mut input = serde_json::Map::new();
        let UpdateParams {
            title,
            description,
            status,
            priority,
            issue_type,
            labels,
            dependencies,
            links: _,
        } = params;

        if let Some(v) = title {
            input.insert("title".to_owned(), serde_json::Value::String(v));
        }
        if let Some(v) = description {
            input.insert("description".to_owned(), serde_json::Value::String(v));
        }
        if let Some(v) = priority {
            input.insert(
                "priority".to_owned(),
                serde_json::Value::Number(map_priority_to_linear(v).into()),
            );
        }
        if let Some(status) = status {
            let state_id = self.find_state_for_status(status).await?;
            input.insert("stateId".to_owned(), serde_json::Value::String(state_id));
        }

        let labels_provided = labels.is_some();
        if labels_provided || issue_type.is_some() || status.is_some() {
            let current = if labels_provided {
                None
            } else {
                Some(self.get(id).await?)
            };

            let mut new_labels = labels.unwrap_or_else(|| {
                current
                    .as_ref()
                    .map(|i| i.labels.clone())
                    .unwrap_or_default()
            });

            if let Some(issue_type) = issue_type {
                new_labels.retain(|l| !l.starts_with("type:"));
                new_labels.push(format!("type:{issue_type}"));
            } else if !labels_provided {
                if let Some(curr) = current.as_ref() {
                    if !curr.issue_type.trim().is_empty() {
                        new_labels.push(format!("type:{}", curr.issue_type));
                    }
                }
            }

            if let Some(status) = status {
                if status == Status::Blocked {
                    if !new_labels.iter().any(|l| l == "blocked") {
                        new_labels.push("blocked".to_owned());
                    }
                } else {
                    new_labels.retain(|l| l != "blocked");
                }
            } else if !labels_provided {
                if let Some(curr) = current.as_ref() {
                    if curr.status == Status::Blocked && !new_labels.iter().any(|l| l == "blocked")
                    {
                        new_labels.push("blocked".to_owned());
                    }
                }
            }

            let mut label_ids = Vec::new();
            for name in new_labels {
                match self.find_label_id(&name).await {
                    Ok(Some(id)) => label_ids.push(id),
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(label = %name, error = %err, "failed to find linear label")
                    }
                }
            }

            input.insert(
                "labelIds".to_owned(),
                serde_json::Value::Array(
                    label_ids
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }

        if let Some(deps) = dependencies {
            if let Some(parent) = deps.first() {
                if parent.trim().is_empty() {
                    input.insert("parentId".to_owned(), serde_json::Value::Null);
                } else {
                    match self.resolve_issue_id(parent).await {
                        Ok(parent_id) => {
                            input.insert(
                                "parentId".to_owned(),
                                serde_json::Value::String(parent_id),
                            );
                        }
                        Err(err) => {
                            tracing::warn!(parent = %parent, error = %err, "failed to resolve linear parent issue");
                        }
                    }
                }
            } else {
                input.insert("parentId".to_owned(), serde_json::Value::Null);
            }
        }

        if input.is_empty() {
            return self.get(id).await;
        }

        let query = r#"
            mutation IssueUpdate($id: String!, $input: IssueUpdateInput!) {
                issueUpdate(id: $id, input: $input) {
                    success
                    issue {
                        identifier
                        title
                        description
                        priority
                        createdAt
                        state { type }
                        labels { nodes { name } }
                        parent { identifier }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "issueUpdate")]
            issue_update: IssueUpdate,
        }
        #[derive(Debug, Deserialize)]
        struct IssueUpdate {
            success: bool,
            issue: LinearIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "id": issue_id,
                    "input": serde_json::Value::Object(input),
                })),
            )
            .await
            .context("linear update issue")?;

        if !data.issue_update.success {
            return Err(anyhow!("linear issue update failed"));
        }

        Ok(to_issue(&data.issue_update.issue))
    }

    pub async fn close(&self, id: &str) -> anyhow::Result<()> {
        let _ = self
            .update(
                id,
                UpdateParams {
                    status: Some(Status::Closed),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn comment(&self, id: &str, body: &str) -> anyhow::Result<()> {
        let issue_id = self.resolve_issue_id(id).await?;

        let query = r#"
            mutation CommentCreate($input: CommentCreateInput!) {
                commentCreate(input: $input) {
                    success
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "commentCreate")]
            comment_create: CommentCreate,
        }
        #[derive(Debug, Deserialize)]
        struct CommentCreate {
            success: bool,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "input": {
                        "issueId": issue_id,
                        "body": body,
                    }
                })),
            )
            .await
            .context("linear create comment")?;

        if !data.comment_create.success {
            return Err(anyhow!("linear comment creation failed"));
        }

        Ok(())
    }

    pub async fn commit(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn resolve_issue_id(&self, id: &str) -> anyhow::Result<String> {
        let trimmed = id.trim();
        if is_uuid(trimmed) {
            return Ok(trimmed.to_owned());
        }

        let query = r#"
            query IssueId($id: String!) {
                issue(id: $id) { id }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            issue: Node,
        }
        #[derive(Debug, Deserialize)]
        struct Node {
            id: String,
        }

        let data: Data = self
            .graphql(query, Some(serde_json::json!({ "id": trimmed })))
            .await
            .context("linear resolve issue id")?;

        Ok(data.issue.id)
    }

    async fn find_state_for_status(&self, status: Status) -> anyhow::Result<String> {
        let query = r#"
            query WorkflowStates {
                workflowStates(first: 50) { nodes { id type } }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "workflowStates")]
            workflow_states: WorkflowStates,
        }
        #[derive(Debug, Deserialize)]
        struct WorkflowStates {
            nodes: Vec<StateNode>,
        }
        #[derive(Debug, Deserialize)]
        struct StateNode {
            id: String,
            #[serde(rename = "type")]
            kind: String,
        }

        let data: Data = self
            .graphql(query, None)
            .await
            .context("linear list workflow states")?;

        let target_types: &[&str] = match status {
            Status::Open => &["unstarted", "started", "backlog"],
            Status::Closed => &["completed", "canceled"],
            Status::Blocked => &["unstarted", "backlog"],
        };

        for target in target_types {
            if let Some(node) = data
                .workflow_states
                .nodes
                .iter()
                .find(|n| n.kind == *target)
            {
                return Ok(node.id.clone());
            }
        }

        Err(anyhow!("no suitable workflow state found for {status:?}"))
    }

    async fn find_label_id(&self, name: &str) -> anyhow::Result<Option<String>> {
        let query = r#"
            query Labels($filter: IssueLabelFilter) {
                issueLabels(filter: $filter, first: 1) { nodes { id } }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "issueLabels")]
            issue_labels: LabelConn,
        }
        #[derive(Debug, Deserialize)]
        struct LabelConn {
            nodes: Vec<LabelNode>,
        }
        #[derive(Debug, Deserialize)]
        struct LabelNode {
            id: String,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "filter": { "name": { "eq": name } }
                })),
            )
            .await
            .context("linear query label")?;

        Ok(data.issue_labels.nodes.into_iter().next().map(|l| l.id))
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> anyhow::Result<T> {
        #[derive(Debug, Serialize)]
        struct GraphqlRequest<'a> {
            query: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            variables: Option<serde_json::Value>,
        }

        #[derive(Debug, Deserialize)]
        struct GraphqlResponse<T> {
            data: Option<T>,
            #[serde(default)]
            errors: Vec<GraphqlError>,
        }

        #[derive(Debug, Deserialize)]
        struct GraphqlError {
            message: String,
        }

        let resp = self
            .client
            .post(&self.graphql_url)
            .header(reqwest::header::AUTHORIZATION, self.api_key.clone())
            .json(&GraphqlRequest { query, variables })
            .send()
            .await
            .context("send request")?;
        let status = resp.status();
        let text = resp.text().await.context("read response")?;

        if !status.is_success() {
            return Err(anyhow!("linear api error ({status}): {text}"));
        }

        let parsed: GraphqlResponse<T> =
            serde_json::from_str(&text).context("parse graphql response")?;
        if let Some(first) = parsed.errors.first() {
            return Err(anyhow!("linear graphql error: {}", first.message));
        }
        parsed
            .data
            .ok_or_else(|| anyhow!("linear graphql response missing data"))
    }
}

#[derive(Debug, Deserialize)]
struct LinearIssue {
    identifier: String,
    title: String,
    #[serde(default)]
    description: String,
    priority: i32,
    #[serde(rename = "createdAt")]
    created_at: String,
    state: LinearState,
    labels: LinearLabels,
    #[serde(default)]
    parent: Option<LinearParent>,
}

#[derive(Debug, Deserialize)]
struct LinearState {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct LinearLabels {
    nodes: Vec<LinearLabel>,
}

#[derive(Debug, Deserialize)]
struct LinearLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct LinearParent {
    identifier: String,
}

fn to_issue(li: &LinearIssue) -> Issue {
    let created_at_ms = parse_rfc3339_ms(&li.created_at).unwrap_or(0);

    let mut issue_type = String::new();
    let mut labels = Vec::new();
    let mut status = match li.state.kind.as_str() {
        "completed" | "canceled" => Status::Closed,
        _ => Status::Open,
    };

    for label in &li.labels.nodes {
        let name = label.name.as_str();
        if let Some(rest) = name.strip_prefix("type:") {
            issue_type = rest.to_owned();
            continue;
        }
        if name == "blocked" {
            status = Status::Blocked;
            continue;
        }
        labels.push(name.to_owned());
    }

    if issue_type.is_empty() {
        issue_type = "task".to_owned();
    }

    let mut dependencies = Vec::new();
    if let Some(parent) = li.parent.as_ref() {
        dependencies.push(parent.identifier.clone());
    }

    Issue {
        id: li.identifier.clone(),
        title: li.title.clone(),
        description: li.description.clone(),
        status,
        priority: map_priority_from_linear(li.priority),
        issue_type,
        dependencies,
        labels,
        links: vec![],
        created_at_ms,
    }
}

fn map_priority_to_linear(priority: i32) -> i64 {
    match priority {
        0 => 4,
        1 => 3,
        2 => 2,
        _ => 0,
    }
}

fn map_priority_from_linear(priority: i32) -> i32 {
    match priority {
        1 | 2 => 2,
        3 => 1,
        4 => 0,
        _ => 1,
    }
}

fn parse_rfc3339_ms(s: &str) -> anyhow::Result<u64> {
    let dt = time::OffsetDateTime::parse(s.trim(), &time::format_description::well_known::Rfc3339)
        .map_err(|e| anyhow!("parse rfc3339: {e}"))?;
    Ok(dt.unix_timestamp_nanos() as u64 / 1_000_000)
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36 && s.matches('-').count() == 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_priority_matches_fab_expectations() {
        assert_eq!(map_priority_to_linear(0), 4);
        assert_eq!(map_priority_to_linear(1), 3);
        assert_eq!(map_priority_to_linear(2), 2);
        assert_eq!(map_priority_to_linear(3), 0);

        assert_eq!(map_priority_from_linear(1), 2);
        assert_eq!(map_priority_from_linear(2), 2);
        assert_eq!(map_priority_from_linear(3), 1);
        assert_eq!(map_priority_from_linear(4), 0);
        assert_eq!(map_priority_from_linear(0), 1);
    }
}
