use murmur::github::GithubBackend;
use murmur_core::issue::{CreateParams, ListFilter, Status, UpdateParams};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn github_issue_node(
    number: i64,
    title: &str,
    state: &str,
    author: &str,
    labels: &[&str],
    blocked_by: &[(i64, &str)],
) -> serde_json::Value {
    json!({
        "id": format!("node-{number}"),
        "number": number,
        "title": title,
        "body": format!("body {number}"),
        "state": state,
        "createdAt": "2026-01-20T00:00:00Z",
        "author": { "login": author },
        "labels": { "nodes": labels.iter().map(|l| json!({"id": format!("lbl-{l}"), "name": l})).collect::<Vec<_>>() },
        "blockedBy": { "nodes": blocked_by.iter().map(|(n,s)| json!({"number": n, "state": s})).collect::<Vec<_>>() },
    })
}

#[tokio::test]
async fn github_list_maps_labels_and_dependencies() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("ListIssues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "repository": {
                    "issues": {
                        "nodes": [
                            github_issue_node(
                                1,
                                "Blocked bug",
                                "OPEN",
                                "owner",
                                &["type:bug", "priority:2", "blocked", "ui"],
                                &[(99, "OPEN")],
                            ),
                            github_issue_node(
                                2,
                                "Closed task",
                                "CLOSED",
                                "owner",
                                &["priority:1"],
                                &[],
                            ),
                        ]
                    }
                }
            }
        })))
        .mount(&server)
        .await;

    let backend = GithubBackend::new(
        "owner".to_owned(),
        "repo".to_owned(),
        "test-token".to_owned(),
        vec![],
        server.uri(),
    )
    .unwrap();

    let issues = backend.list(ListFilter::default()).await.unwrap();
    assert_eq!(issues.len(), 2);

    let a = &issues[0];
    assert_eq!(a.id, "1");
    assert_eq!(a.status, Status::Blocked);
    assert_eq!(a.issue_type, "bug");
    assert_eq!(a.priority, 2);
    assert_eq!(a.labels, vec!["ui".to_owned()]);
    assert_eq!(a.dependencies, vec!["99".to_owned()]);
    assert!(a.created_at_ms > 0);

    let b = &issues[1];
    assert_eq!(b.id, "2");
    assert_eq!(b.status, Status::Closed);
    assert_eq!(b.issue_type, "task");
    assert_eq!(b.priority, 1);
    assert!(b.labels.is_empty());
}

#[tokio::test]
async fn github_ready_filters_blocked_authors_and_open_blockers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("ListIssuesForReady"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "repository": {
                    "issues": {
                        "nodes": [
                            github_issue_node(1, "open blocker", "OPEN", "owner", &["priority:1"], &[(9, "OPEN")]),
                            github_issue_node(2, "blocked", "OPEN", "owner", &["blocked"], &[]),
                            github_issue_node(3, "wrong author", "OPEN", "someone", &[], &[]),
                            github_issue_node(4, "ready", "OPEN", "owner", &[], &[(10, "CLOSED")]),
                        ]
                    }
                }
            }
        })))
        .mount(&server)
        .await;

    let backend = GithubBackend::new(
        "owner".to_owned(),
        "repo".to_owned(),
        "test-token".to_owned(),
        vec![],
        server.uri(),
    )
    .unwrap();

    let issues = backend.ready().await.unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].id, "4");
}

#[tokio::test]
async fn github_create_update_close_and_comment_smoke() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("GetRepositoryID"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "repository": { "id": "repo-123" } }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("GetLabel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "repository": { "label": { "id": "lbl-1" } } }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("CreateIssue"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "createIssue": {
                    "issue": github_issue_node(7, "Created", "OPEN", "owner", &["type:task", "priority:1", "ui"], &[])
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("GetIssueForUpdate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "repository": {
                    "issue": github_issue_node(7, "Created", "OPEN", "owner", &["type:task", "priority:1", "ui"], &[])
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("UpdateIssue"))
        .and(body_string_contains("\"title\":\"Renamed\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "updateIssue": {
                    "issue": github_issue_node(7, "Renamed", "OPEN", "owner", &["type:task", "priority:1", "ui"], &[])
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("UpdateIssue"))
        .and(body_string_contains("\"state\":\"CLOSED\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "updateIssue": {
                    "issue": github_issue_node(7, "Renamed", "CLOSED", "owner", &["type:task", "priority:1", "ui"], &[])
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("GetIssueNodeID"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "repository": {
                    "issue": { "id": "node-7" }
                }
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("AddIssueComment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "addComment": {
                    "commentEdge": { "node": { "id": "c-1" } }
                }
            }
        })))
        .mount(&server)
        .await;

    let backend = GithubBackend::new(
        "owner".to_owned(),
        "repo".to_owned(),
        "test-token".to_owned(),
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
    assert_eq!(created.id, "7");

    let updated = backend
        .update(
            "7",
            UpdateParams {
                title: Some("Renamed".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "Renamed");

    backend.close("7").await.unwrap();
    backend.comment("7", "hello").await.unwrap();
}
