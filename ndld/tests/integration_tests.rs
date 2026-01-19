use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ndld::{
    auth::{OAuthConfig, SessionStore},
    routes::{AppState, create_router},
};
use std::sync::Arc;
use tower::ServiceExt;

fn create_test_state() -> Arc<AppState> {
    Arc::new(AppState {
        sessions: SessionStore::new(),
        oauth: OAuthConfig {
            client_id: "test_client_id".to_string(),
            client_secret: "test_client_secret".to_string(),
            public_url: "https://test.example.com".to_string(),
        },
    })
}

#[tokio::test]
async fn test_health_endpoint() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
    assert!(json["git"].is_string());
}

#[tokio::test]
async fn test_index_page() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("ndld"));
    assert!(html.contains("OAuth"));
}

#[tokio::test]
async fn test_start_auth() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/start")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["session_id"].is_string());
    assert!(json["auth_url"].is_string());

    let auth_url = json["auth_url"].as_str().unwrap();
    assert!(auth_url.contains("threads.net/oauth/authorize"));
    assert!(auth_url.contains("client_id=test_client_id"));
    assert!(auth_url.contains("redirect_uri="));
}

#[tokio::test]
async fn test_poll_pending_session() {
    let state = create_test_state();

    // Create a session first
    let session = state.sessions.create_session();
    let session_id = session.id.clone();

    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/auth/poll/{}", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "pending");
}

#[tokio::test]
async fn test_poll_nonexistent_session() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/poll/nonexistent-session-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_callback_missing_state() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/callback?code=test_code")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK); // Returns HTML error page

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Missing state parameter"));
}

#[tokio::test]
async fn test_callback_invalid_session() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/callback?code=test_code&state=invalid-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK); // Returns HTML error page

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Session not found"));
}

#[tokio::test]
async fn test_callback_oauth_error() {
    let state = create_test_state();

    // Create a session first
    let session = state.sessions.create_session();
    let session_id = session.id.clone();

    let app = create_router(Arc::clone(&state));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/auth/callback?error=access_denied&error_description=User%20denied%20access&state={}",
                    session_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK); // Returns HTML error page

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("User denied access"));

    // Verify session state was updated to failed
    let app = create_router(state);
    let poll_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/auth/poll/{}", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(poll_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "failed");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("User denied access")
    );
}

#[tokio::test]
async fn test_privacy_policy_page() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/privacy-policy")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Privacy Policy"));
}

#[tokio::test]
async fn test_tos_page() {
    let state = create_test_state();
    let app = create_router(state);

    let response = app
        .oneshot(Request::builder().uri("/tos").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Terms of Service"));
}
