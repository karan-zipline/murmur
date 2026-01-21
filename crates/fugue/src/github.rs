use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{anyhow, Context as _};
use fugue_core::issue::{CreateParams, Issue, ListFilter, Status, UpdateParams};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::git::Git;

#[derive(Debug, Clone)]
pub struct GithubBackend {
    client: reqwest::Client,
    graphql_url: String,
    token: String,
    owner: String,
    repo: String,
    allowed_authors: Vec<String>,
}

impl GithubBackend {
    pub fn new(
        owner: String,
        repo: String,
        token: String,
        mut allowed_authors: Vec<String>,
        graphql_url: String,
    ) -> anyhow::Result<Self> {
        if owner.trim().is_empty() || repo.trim().is_empty() {
            return Err(anyhow!("invalid github repo: {owner}/{repo}"));
        }
        if token.trim().is_empty() {
            return Err(anyhow!("missing github token"));
        }

        if allowed_authors.is_empty() {
            allowed_authors.push(owner.clone());
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
            token,
            owner,
            repo,
            allowed_authors,
        })
    }

    pub async fn from_repo(
        git: &Git,
        repo_dir: &Path,
        token: String,
        allowed_authors: Vec<String>,
        graphql_url: String,
    ) -> anyhow::Result<Self> {
        let remote = git
            .remote_origin_url(repo_dir)
            .await
            .context("git remote get-url origin")?;

        let (owner, repo) =
            parse_github_nwo(&remote).ok_or_else(|| anyhow!("not a github remote: {remote}"))?;

        Self::new(owner, repo, token, allowed_authors, graphql_url)
    }

    pub fn nwo(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Issue> {
        let num = parse_issue_number(id)?;

        let query = r#"
            query GetIssue($owner: String!, $repo: String!, $number: Int!) {
                repository(owner: $owner, name: $repo) {
                    issue(number: $number) {
                        id
                        number
                        title
                        body
                        state
                        createdAt
                        author { login }
                        labels(first: 100) { nodes { name } }
                        blockedBy(first: 50) { nodes { number state } }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            issue: GithubIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                    "number": num,
                })),
                None,
            )
            .await
            .context("github get issue")?;

        Ok(to_issue(&data.repository.issue))
    }

    pub async fn list(&self, filter: ListFilter) -> anyhow::Result<Vec<Issue>> {
        let mut states = Vec::new();
        if !filter.status.is_empty() {
            for s in &filter.status {
                match s {
                    Status::Open | Status::Blocked => states.push("OPEN"),
                    Status::Closed => states.push("CLOSED"),
                }
            }
        }
        let states = {
            let set: BTreeSet<&str> = states.into_iter().collect();
            set.into_iter().collect::<Vec<_>>()
        };

        let query = r#"
            query ListIssues($owner: String!, $repo: String!, $states: [IssueState!], $first: Int!) {
                repository(owner: $owner, name: $repo) {
                    issues(states: $states, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
                        nodes {
                            id
                            number
                            title
                            body
                            state
                            createdAt
                            author { login }
                            labels(first: 100) { nodes { name } }
                            blockedBy(first: 50) { nodes { number state } }
                        }
                    }
                }
            }
        "#;

        let variables = if states.is_empty() {
            serde_json::json!({
                "owner": self.owner,
                "repo": self.repo,
                "first": 100,
            })
        } else {
            serde_json::json!({
                "owner": self.owner,
                "repo": self.repo,
                "states": states,
                "first": 100,
            })
        };

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            issues: IssueConn,
        }
        #[derive(Debug, Deserialize)]
        struct IssueConn {
            nodes: Vec<GithubIssue>,
        }

        let data: Data = self
            .graphql(query, Some(variables), None)
            .await
            .context("github list issues")?;

        let mut out = Vec::new();
        for iss in data.repository.issues.nodes {
            if !filter.labels.is_empty() {
                let labels: BTreeSet<&str> =
                    iss.labels.nodes.iter().map(|l| l.name.as_str()).collect();
                if !filter.labels.iter().all(|l| labels.contains(l.as_str())) {
                    continue;
                }
            }
            out.push(to_issue(&iss));
        }
        Ok(out)
    }

    pub async fn ready(&self) -> anyhow::Result<Vec<Issue>> {
        let allowed: BTreeSet<String> = self
            .allowed_authors
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let query = r#"
            query ListIssuesForReady($owner: String!, $repo: String!) {
                repository(owner: $owner, name: $repo) {
                    issues(states: [OPEN], first: 100, orderBy: {field: UPDATED_AT, direction: DESC}) {
                        nodes {
                            id
                            number
                            title
                            body
                            state
                            createdAt
                            author { login }
                            labels(first: 100) { nodes { name } }
                            blockedBy(first: 50) { nodes { number state } }
                        }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            issues: IssueConn,
        }
        #[derive(Debug, Deserialize)]
        struct IssueConn {
            nodes: Vec<GithubIssue>,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                })),
                None,
            )
            .await
            .context("github ready query")?;

        let mut ready = Vec::new();
        for gh in data.repository.issues.nodes {
            let iss = to_issue(&gh);
            if iss.status == Status::Blocked {
                continue;
            }

            if !allowed.is_empty() {
                let is_allowed = gh
                    .author
                    .as_ref()
                    .is_some_and(|a| allowed.contains(&a.login.trim().to_ascii_lowercase()));
                if !is_allowed {
                    continue;
                }
            }

            if gh
                .blocked_by
                .as_ref()
                .is_some_and(|b| b.nodes.iter().any(|n| n.state == "OPEN"))
            {
                continue;
            }

            ready.push(iss);
        }

        Ok(ready)
    }

    pub async fn create(&self, params: CreateParams) -> anyhow::Result<Issue> {
        if !params.links.is_empty() {
            return Err(anyhow!("github backend does not support `links`"));
        }

        let repo_id = self.get_repository_id().await?;

        let issue_type = if params.issue_type.trim().is_empty() {
            "task".to_owned()
        } else {
            params.issue_type
        };

        let mut label_names = Vec::new();
        label_names.push(format!("type:{issue_type}"));
        label_names.push(format!("priority:{}", params.priority));
        label_names.extend(params.labels);

        let mut label_ids = Vec::new();
        for name in label_names {
            match self.find_or_create_label(&name).await {
                Ok(Some(id)) => label_ids.push(id),
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(label = %name, error = %err, "failed to find/create github label")
                }
            }
        }

        let query = r#"
            mutation CreateIssue($input: CreateIssueInput!) {
                createIssue(input: $input) {
                    issue {
                        id
                        number
                        title
                        body
                        state
                        createdAt
                        author { login }
                        labels(first: 100) { nodes { name } }
                        blockedBy(first: 50) { nodes { number state } }
                    }
                }
            }
        "#;

        let mut input = serde_json::Map::new();
        input.insert(
            "repositoryId".to_owned(),
            serde_json::Value::String(repo_id),
        );
        input.insert("title".to_owned(), serde_json::Value::String(params.title));
        if !params.description.trim().is_empty() {
            input.insert(
                "body".to_owned(),
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

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "createIssue")]
            create_issue: CreateIssue,
        }
        #[derive(Debug, Deserialize)]
        struct CreateIssue {
            issue: GithubIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({ "input": serde_json::Value::Object(input) })),
                None,
            )
            .await
            .context("github create issue")?;

        for dep in params.dependencies {
            if let Err(err) = self
                .add_blocked_by(&data.create_issue.issue.number.to_string(), &dep)
                .await
            {
                tracing::warn!(dep = %dep, error = %err, "failed to add github dependency");
            }
        }

        Ok(to_issue(&data.create_issue.issue))
    }

    pub async fn update(&self, id: &str, params: UpdateParams) -> anyhow::Result<Issue> {
        if params.dependencies.is_some() {
            return Err(anyhow!(
                "github backend does not support updating dependencies"
            ));
        }
        if params.links.is_some() {
            return Err(anyhow!("github backend does not support `links`"));
        }

        let UpdateParams {
            title,
            description,
            status,
            priority,
            issue_type,
            labels,
            dependencies: _,
            links: _,
        } = params;

        let current = self.get_issue_api(id).await?;
        let current_issue = to_issue(&current);

        let mut input = serde_json::Map::new();
        input.insert(
            "id".to_owned(),
            serde_json::Value::String(current.id.clone()),
        );

        if let Some(v) = title {
            input.insert("title".to_owned(), serde_json::Value::String(v));
        }
        if let Some(v) = description {
            input.insert("body".to_owned(), serde_json::Value::String(v));
        }

        if let Some(status) = status {
            let state = match status {
                Status::Closed => "CLOSED",
                Status::Open | Status::Blocked => "OPEN",
            };
            input.insert(
                "state".to_owned(),
                serde_json::Value::String(state.to_owned()),
            );
        }

        let labels_provided = labels.is_some();
        if labels_provided || issue_type.is_some() || priority.is_some() || status.is_some() {
            let mut new_labels = if let Some(v) = labels {
                v
            } else {
                current_issue.labels.clone()
            };

            if let Some(v) = issue_type {
                new_labels.retain(|l| !l.starts_with("type:"));
                new_labels.push(format!("type:{v}"));
            } else if !current_issue.issue_type.trim().is_empty() && !labels_provided {
                new_labels.push(format!("type:{}", current_issue.issue_type));
            }

            if let Some(v) = priority {
                new_labels.retain(|l| !l.starts_with("priority:"));
                new_labels.push(format!("priority:{v}"));
            } else if !labels_provided {
                new_labels.push(format!("priority:{}", current_issue.priority));
            }

            if let Some(status) = status {
                if status == Status::Blocked {
                    if !new_labels.iter().any(|l| l == "blocked") {
                        new_labels.push("blocked".to_owned());
                    }
                } else {
                    new_labels.retain(|l| l != "blocked");
                }
            }

            let mut label_ids = Vec::new();
            for name in new_labels {
                match self.find_or_create_label(&name).await {
                    Ok(Some(id)) => label_ids.push(id),
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(label = %name, error = %err, "failed to find/create github label")
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

        let query = r#"
            mutation UpdateIssue($input: UpdateIssueInput!) {
                updateIssue(input: $input) {
                    issue {
                        id
                        number
                        title
                        body
                        state
                        createdAt
                        author { login }
                        labels(first: 100) { nodes { name } }
                        blockedBy(first: 50) { nodes { number state } }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "updateIssue")]
            update_issue: UpdateIssue,
        }
        #[derive(Debug, Deserialize)]
        struct UpdateIssue {
            issue: GithubIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({ "input": serde_json::Value::Object(input) })),
                None,
            )
            .await
            .context("github update issue")?;

        Ok(to_issue(&data.update_issue.issue))
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
        let issue = self.get_issue_node_id(id).await?;

        let query = r#"
            mutation AddIssueComment($input: AddCommentInput!) {
                addComment(input: $input) {
                    commentEdge { node { id } }
                }
            }
        "#;

        let _: serde_json::Value = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "input": {
                        "subjectId": issue,
                        "body": body,
                    }
                })),
                None,
            )
            .await
            .context("github add comment")?;

        Ok(())
    }

    pub async fn create_pull_request(
        &self,
        base_branch: &str,
        head_branch: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let base = base_branch.trim();
        let head = head_branch.trim();
        if base.is_empty() || head.is_empty() {
            return Err(anyhow!("pull request base/head is empty"));
        }

        let repo_id = self.get_repository_id().await?;

        let query = r#"
            mutation CreatePullRequest($input: CreatePullRequestInput!) {
                createPullRequest(input: $input) {
                    pullRequest { url }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "createPullRequest")]
            create_pull_request: CreatePullRequest,
        }
        #[derive(Debug, Deserialize)]
        struct CreatePullRequest {
            #[serde(rename = "pullRequest")]
            pull_request: PullRequest,
        }
        #[derive(Debug, Deserialize)]
        struct PullRequest {
            url: String,
        }

        let input = serde_json::json!({
            "repositoryId": repo_id,
            "baseRefName": base,
            "headRefName": head,
            "title": title,
            "body": body,
        });

        let data: Data = self
            .graphql(query, Some(serde_json::json!({ "input": input })), None)
            .await
            .context("github create pull request")?;

        Ok(data.create_pull_request.pull_request.url)
    }

    pub async fn commit(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_repository_id(&self) -> anyhow::Result<String> {
        let query = r#"
            query GetRepositoryID($owner: String!, $repo: String!) {
                repository(owner: $owner, name: $repo) { id }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            id: String,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                })),
                None,
            )
            .await
            .context("github get repo id")?;

        Ok(data.repository.id)
    }

    async fn find_or_create_label(&self, name: &str) -> anyhow::Result<Option<String>> {
        if let Some(id) = self.get_label_id(name).await? {
            return Ok(Some(id));
        }

        match self.create_label(name).await {
            Ok(Some(id)) => Ok(Some(id)),
            Ok(None) => Ok(None),
            Err(err) => {
                if err.to_string().contains("already exists") {
                    return self.get_label_id(name).await;
                }
                Err(err)
            }
        }
    }

    async fn get_label_id(&self, name: &str) -> anyhow::Result<Option<String>> {
        let query = r#"
            query GetLabel($owner: String!, $repo: String!, $name: String!) {
                repository(owner: $owner, name: $repo) {
                    label(name: $name) { id }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            label: Option<Label>,
        }
        #[derive(Debug, Deserialize)]
        struct Label {
            id: String,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                    "name": name,
                })),
                None,
            )
            .await
            .context("github get label")?;

        Ok(data.repository.label.map(|l| l.id))
    }

    async fn create_label(&self, name: &str) -> anyhow::Result<Option<String>> {
        let repo_id = self.get_repository_id().await?;

        let color = match name {
            s if s.starts_with("type:") => "0366d6",
            s if s.starts_with("priority:") => "fbca04",
            "blocked" => "d73a4a",
            _ => "ededed",
        };

        let query = r#"
            mutation CreateLabel($input: CreateLabelInput!) {
                createLabel(input: $input) { label { id } }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "createLabel")]
            create_label: CreateLabel,
        }
        #[derive(Debug, Deserialize)]
        struct CreateLabel {
            label: Label,
        }
        #[derive(Debug, Deserialize)]
        struct Label {
            id: String,
        }

        let resp: anyhow::Result<Data> = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "input": {
                        "repositoryId": repo_id,
                        "name": name,
                        "color": color,
                    }
                })),
                None,
            )
            .await;

        resp.map(|data| Some(data.create_label.label.id))
    }

    async fn add_blocked_by(
        &self,
        blocked_issue_num: &str,
        blocking_issue_num: &str,
    ) -> anyhow::Result<()> {
        let blocked_issue_id = self.get_issue_node_id(blocked_issue_num).await?;
        let blocking_issue_id = self.get_issue_node_id(blocking_issue_num).await?;

        let query = r#"
            mutation AddBlockedBy($input: AddBlockedByInput!) {
                addBlockedBy(input: $input) {
                    issue { number }
                    blockingIssue { number }
                }
            }
        "#;

        let _: serde_json::Value = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "input": {
                        "issueId": blocked_issue_id,
                        "blockingIssueId": blocking_issue_id,
                    }
                })),
                None,
            )
            .await
            .context("github add blockedBy")?;

        Ok(())
    }

    async fn get_issue_node_id(&self, id: &str) -> anyhow::Result<String> {
        let num = parse_issue_number(id)?;

        let query = r#"
            query GetIssueNodeID($owner: String!, $repo: String!, $number: Int!) {
                repository(owner: $owner, name: $repo) {
                    issue(number: $number) { id }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            issue: Node,
        }
        #[derive(Debug, Deserialize)]
        struct Node {
            id: String,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                    "number": num,
                })),
                None,
            )
            .await
            .context("github get issue node id")?;

        Ok(data.repository.issue.id)
    }

    async fn get_issue_api(&self, id: &str) -> anyhow::Result<GithubIssue> {
        let num = parse_issue_number(id)?;

        let query = r#"
            query GetIssueForUpdate($owner: String!, $repo: String!, $number: Int!) {
                repository(owner: $owner, name: $repo) {
                    issue(number: $number) {
                        id
                        number
                        title
                        body
                        state
                        createdAt
                        author { login }
                        labels(first: 100) { nodes { name } }
                        blockedBy(first: 50) { nodes { number state } }
                    }
                }
            }
        "#;

        #[derive(Debug, Deserialize)]
        struct Data {
            repository: Repository,
        }
        #[derive(Debug, Deserialize)]
        struct Repository {
            issue: GithubIssue,
        }

        let data: Data = self
            .graphql(
                query,
                Some(serde_json::json!({
                    "owner": self.owner,
                    "repo": self.repo,
                    "number": num,
                })),
                None,
            )
            .await
            .context("github get issue for update")?;

        Ok(data.repository.issue)
    }

    async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
        features: Option<&[&str]>,
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

        let mut req = self
            .client
            .post(&self.graphql_url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .json(&GraphqlRequest { query, variables });

        if let Some(features) = features {
            if !features.is_empty() {
                req = req.header("GraphQL-Features", features.join(","));
            }
        }

        let resp = req.send().await.context("send request")?;
        let status = resp.status();
        let text = resp.text().await.context("read response")?;

        if !status.is_success() {
            return Err(anyhow!("github api error ({status}): {text}"));
        }

        let parsed: GraphqlResponse<T> =
            serde_json::from_str(&text).context("parse graphql response")?;
        if let Some(first) = parsed.errors.first() {
            return Err(anyhow!("github graphql error: {}", first.message));
        }
        parsed
            .data
            .ok_or_else(|| anyhow!("github graphql response missing data"))
    }
}

#[derive(Debug, Deserialize)]
struct GithubIssue {
    id: String,
    number: i64,
    title: String,
    #[serde(default)]
    body: String,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(default)]
    author: Option<GithubAuthor>,
    labels: GithubLabels,
    #[serde(default, rename = "blockedBy")]
    blocked_by: Option<GithubBlockedBy>,
}

#[derive(Debug, Deserialize)]
struct GithubAuthor {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubLabels {
    nodes: Vec<GithubLabel>,
}

#[derive(Debug, Deserialize)]
struct GithubLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GithubBlockedBy {
    nodes: Vec<GithubBlockedByNode>,
}

#[derive(Debug, Deserialize)]
struct GithubBlockedByNode {
    number: i64,
    state: String,
}

fn to_issue(gh: &GithubIssue) -> Issue {
    let created_at_ms = parse_rfc3339_ms(&gh.created_at).unwrap_or(0);

    let mut issue_type = String::new();
    let mut priority = 0;
    let mut labels = Vec::new();

    let mut status = if gh.state == "CLOSED" {
        Status::Closed
    } else {
        Status::Open
    };

    for label in &gh.labels.nodes {
        let name = label.name.as_str();
        if let Some(rest) = name.strip_prefix("type:") {
            issue_type = rest.to_owned();
            continue;
        }
        if let Some(rest) = name.strip_prefix("priority:") {
            priority = rest.parse::<i32>().unwrap_or(priority);
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

    let dependencies = gh
        .blocked_by
        .as_ref()
        .map(|b| {
            b.nodes
                .iter()
                .map(|n| n.number.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Issue {
        id: gh.number.to_string(),
        title: gh.title.clone(),
        description: gh.body.clone(),
        status,
        priority,
        issue_type,
        dependencies,
        labels,
        links: vec![],
        created_at_ms,
    }
}

fn parse_rfc3339_ms(s: &str) -> anyhow::Result<u64> {
    let dt = time::OffsetDateTime::parse(s.trim(), &time::format_description::well_known::Rfc3339)
        .map_err(|e| anyhow!("parse rfc3339: {e}"))?;
    Ok(dt.unix_timestamp_nanos() as u64 / 1_000_000)
}

fn parse_issue_number(s: &str) -> anyhow::Result<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("invalid issue id: empty"));
    }
    trimmed
        .parse::<i64>()
        .map_err(|_| anyhow!("invalid github issue number: {trimmed}"))
}

pub fn parse_github_nwo(remote_url: &str) -> Option<(String, String)> {
    let url = remote_url.trim();

    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return split_owner_repo(rest);
    }

    if let Ok(parsed) = Url::parse(url) {
        if parsed.host_str()? != "github.com" {
            return None;
        }
        return split_owner_repo(parsed.path().trim_start_matches('/'));
    }

    None
}

fn split_owner_repo(path: &str) -> Option<(String, String)> {
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    if parts.next().is_some() {
        return None;
    }
    Some((owner.to_owned(), repo.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_nwo_parses_expected_variants() {
        assert_eq!(
            parse_github_nwo("git@github.com:owner/repo.git"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(
            parse_github_nwo("https://github.com/owner/repo.git"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(
            parse_github_nwo("https://github.com/owner/repo"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(
            parse_github_nwo("ssh://git@github.com/owner/repo.git"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(parse_github_nwo("git@gitlab.com:owner/repo.git"), None);
        assert_eq!(parse_github_nwo("not-a-url"), None);
    }
}
