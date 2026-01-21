use fugue::linear::LinearBackend;
use fugue_core::issue::{CreateParams, ListFilter, Status, UpdateParams};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn linear_issue_node(
    identifier: &str,
    title: &str,
    state_type: &str,
    priority: i32,
    labels: &[&str],
    parent: Option<&str>,
) -> serde_json::Value {
    json!({
        "identifier": identifier,
        "title": title,
        "description": format!("desc {identifier}"),
        "priority": priority,
        "createdAt": "2026-01-20T00:00:00Z",
        "state": { "type": state_type },
        "labels": { "nodes": labels.iter().map(|n| json!({"name": n})).collect::<Vec<_>>() },
        "parent": parent.map(|p| json!({"identifier": p})).unwrap_or(serde_json::Value::Null),
    })
}

#[tokio::test]
async fn linear_list_maps_priority_type_blocked_and_parent() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query Issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issues": {
                    "nodes": [
                        linear_issue_node("ENG-1", "Blocked bug", "started", 2, &["type:bug", "blocked", "ui"], Some("ENG-0")),
                        linear_issue_node("ENG-2", "Closed", "completed", 4, &[], None),
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    let backend = LinearBackend::new(
        "team-1".to_owned(),
        None,
        "lin-key".to_owned(),
        vec![],
        server.uri(),
    )
    .unwrap();

    let issues = backend.list(ListFilter::default()).await.unwrap();
    assert_eq!(issues.len(), 2);

    let a = &issues[0];
    assert_eq!(a.id, "ENG-1");
    assert_eq!(a.status, Status::Blocked);
    assert_eq!(a.issue_type, "bug");
    assert_eq!(a.priority, 2);
    assert_eq!(a.labels, vec!["ui".to_owned()]);
    assert_eq!(a.dependencies, vec!["ENG-0".to_owned()]);
    assert!(a.created_at_ms > 0);

    let b = &issues[1];
    assert_eq!(b.id, "ENG-2");
    assert_eq!(b.status, Status::Closed);
    assert_eq!(b.issue_type, "task");
    assert_eq!(b.priority, 0);
}

#[tokio::test]
async fn linear_ready_filters_blocked_and_open_deps() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query Issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issues": {
                    "nodes": [
                        linear_issue_node("ENG-1", "A", "backlog", 3, &[], None),
                        linear_issue_node("ENG-2", "Blocked", "backlog", 3, &["blocked"], None),
                        linear_issue_node("ENG-3", "Depends on ENG-1", "backlog", 3, &[], Some("ENG-1")),
                        linear_issue_node("ENG-4", "Depends on closed", "backlog", 3, &[], Some("ENG-X")),
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    let backend = LinearBackend::new(
        "team-1".to_owned(),
        None,
        "lin-key".to_owned(),
        vec![],
        server.uri(),
    )
    .unwrap();

    let ready = backend.ready().await.unwrap();
    let ids = ready.into_iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ids, vec!["ENG-1".to_owned(), "ENG-4".to_owned()]);
}

#[tokio::test]
async fn linear_create_update_close_and_comment_smoke() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query Labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "issueLabels": { "nodes": [ { "id": "lbl-1" } ] } }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("mutation IssueCreate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issueCreate": {
                    "success": true,
                    "issue": linear_issue_node("ENG-10", "Created", "backlog", 3, &["type:task", "ui"], None)
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query IssueId"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "issue": { "id": "uuid-10" } }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("mutation IssueUpdate"))
        .and(body_string_contains("\"title\":\"Renamed\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": linear_issue_node("ENG-10", "Renamed", "backlog", 3, &["type:task", "ui"], None)
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query WorkflowStates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "workflowStates": {
                    "nodes": [
                        { "id": "state-done", "type": "completed" }
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query Issue($id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issue": linear_issue_node("ENG-10", "Renamed", "backlog", 3, &["type:task", "ui"], None)
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("mutation IssueUpdate"))
        .and(body_string_contains("\"stateId\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issueUpdate": {
                    "success": true,
                    "issue": linear_issue_node("ENG-10", "Renamed", "completed", 3, &["type:task", "ui"], None)
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("mutation CommentCreate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "commentCreate": { "success": true } }
        })))
        .mount(&server)
        .await;

    let backend = LinearBackend::new(
        "team-1".to_owned(),
        None,
        "lin-key".to_owned(),
        vec![],
        server.uri(),
    )
    .unwrap();

    let created = backend
        .create(CreateParams {
            title: "Created".to_owned(),
            description: "hello".to_owned(),
            issue_type: "task".to_owned(),
            priority: 1,
            labels: vec!["ui".to_owned()],
            dependencies: vec![],
            links: vec![],
        })
        .await
        .unwrap();
    assert_eq!(created.id, "ENG-10");

    let updated = backend
        .update(
            "ENG-10",
            UpdateParams {
                title: Some("Renamed".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "Renamed");

    backend.close("ENG-10").await.unwrap();
    backend.comment("ENG-10", "hello").await.unwrap();
}
