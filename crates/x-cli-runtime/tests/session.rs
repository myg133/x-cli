//! Session 的集成测试 —— 含 mock HTTP server 跑真实 login 流程

use serde_json::json;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use x_cli_core::{
    AuthConfig, LoginConfig, LoginRequest, LoginResponse, RefreshConfig, TokenSource,
};
use x_cli_runtime::{HttpCaller, Session};

// ========== from_cli_flags 测试（来自原 build_auth_profile） ==========

#[tokio::test]
async fn empty_inputs_produces_empty_headers() {
    let s = Session::from_cli_flags(&[], &[]).expect("build");
    assert!(s.headers().await.is_empty());
}

#[tokio::test]
async fn bearer_token_becomes_authorization_header() {
    let s = Session::from_cli_flags(&["abc123".to_string()], &[]).expect("build");
    let h = s.headers().await;
    assert_eq!(
        h.get("Authorization").map(String::as_str),
        Some("Bearer abc123")
    );
}

#[tokio::test]
async fn multiple_bearers_last_wins() {
    let s =
        Session::from_cli_flags(&["first".to_string(), "second".to_string()], &[]).expect("build");
    let h = s.headers().await;
    assert_eq!(
        h.get("Authorization").map(String::as_str),
        Some("Bearer second")
    );
}

#[tokio::test]
async fn custom_header_passthrough() {
    let s = Session::from_cli_flags(
        &[],
        &["X-API-Key=secret".to_string(), "X-Tenant=acme".to_string()],
    )
    .expect("build");
    let h = s.headers().await;
    assert_eq!(h.get("X-API-Key").map(String::as_str), Some("secret"));
    assert_eq!(h.get("X-Tenant").map(String::as_str), Some("acme"));
}

#[tokio::test]
async fn malformed_header_string_rejected() {
    let err = Session::from_cli_flags(&[], &["NO-EQUALS".to_string()]).unwrap_err();
    assert!(err.to_string().contains("KEY=VALUE"));
}

#[tokio::test]
async fn empty_key_rejected() {
    let err = Session::from_cli_flags(&[], &["=value".to_string()]).unwrap_err();
    assert!(err.to_string().contains("key 不能为空"));
}

// ========== login flow 端到端测试 ==========

/// 启动一个本地 mock HTTP server,记下收到的请求并返回指定响应
async fn spawn_mock_server(
    responses: Vec<(u16, String)>,
) -> (SocketAddr, Arc<Mutex<Vec<(String, String, String)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let requests: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let requests_inner = requests.clone();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_inner = counter.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            let requests = requests_inner.clone();
            let counter = counter_inner.clone();
            let responses = responses.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let n = match socket.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                // 解析 method + path + body（极简:按 \r\n\r\n 切）
                let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
                let mut lines = head.lines();
                let req_line = lines.next().unwrap_or("");
                let method = req_line.split_whitespace().next().unwrap_or("").to_string();
                let path = req_line.split_whitespace().nth(1).unwrap_or("").to_string();
                requests
                    .lock()
                    .await
                    .push((method.clone(), path, body.to_string()));
                let idx = counter.fetch_add(1, Ordering::SeqCst);
                let (status, body_resp) = if idx < responses.len() {
                    responses[idx].clone()
                } else {
                    (404, "{}".to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body_resp.len(),
                    body_resp
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (addr, requests)
}

fn login_cfg(url: String, token_path: &str) -> AuthConfig {
    AuthConfig {
        version: 1,
        token: TokenSource::Login {
            login: Box::new(LoginConfig {
                request: LoginRequest {
                    url: Some(url),
                    method: "POST".to_string(),
                    headers: serde_json::Map::new(),
                    body: json!({ "username": "admin", "password": "secret" }),
                },
                response: LoginResponse {
                    token_path: token_path.to_string(),
                    expires_in_path: None,
                    refresh_token_path: None,
                },
                refresh: RefreshConfig {
                    on_401: true,
                    proactive: false,
                },
            }),
        },
    }
}

#[tokio::test]
async fn from_config_login_does_initial_login() {
    let (addr, requests) = spawn_mock_server(vec![(
        200,
        r#"{"access_token":"initial-token"}"#.to_string(),
    )])
    .await;
    let cfg = login_cfg(format!("http://{addr}/login"), "access_token");
    let s = Session::from_config(cfg, None).await.expect("from_config");
    assert_eq!(
        s.headers().await.get("Authorization").map(String::as_str),
        Some("Bearer initial-token")
    );
    // 验证 login 请求确实发出
    let reqs = requests.lock().await;
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].0, "POST");
    assert_eq!(reqs[0].1, "/login");
    assert!(reqs[0].2.contains("admin"));
}

#[tokio::test]
async fn from_config_login_nested_token_path() {
    let (addr, _) = spawn_mock_server(vec![(
        200,
        r#"{"data":{"access_token":"nested-tok"}}"#.to_string(),
    )])
    .await;
    let cfg = login_cfg(format!("http://{addr}/auth/login"), "data.access_token");
    let s = Session::from_config(cfg, None).await.expect("from_config");
    assert_eq!(
        s.headers().await.get("Authorization").map(String::as_str),
        Some("Bearer nested-tok")
    );
}

#[tokio::test]
async fn from_config_login_non_2xx_returns_error() {
    let (addr, _) = spawn_mock_server(vec![(401, r#"{"error":"bad creds"}"#.to_string())]).await;
    let cfg = login_cfg(format!("http://{addr}/login"), "access_token");
    let err = Session::from_config(cfg, None).await.unwrap_err();
    assert!(err.to_string().contains("401"), "got: {err}");
}

#[tokio::test]
async fn from_config_login_token_path_missing_returns_error() {
    let (addr, _) = spawn_mock_server(vec![(200, r#"{"wrong":"field"}"#.to_string())]).await;
    let cfg = login_cfg(format!("http://{addr}/login"), "access_token");
    let err = Session::from_config(cfg, None).await.unwrap_err();
    assert!(err.to_string().contains("access_token"), "got: {err}");
}

#[tokio::test]
async fn handle_401_static_returns_false() {
    let s = Session::from_cli_flags(&["abc".to_string()], &[]).unwrap();
    assert!(!s.handle_401().await.unwrap());
}

#[tokio::test]
async fn handle_401_empty_returns_false() {
    let s = Session::empty();
    assert!(!s.handle_401().await.unwrap());
}

#[tokio::test]
async fn handle_401_login_success_refreshes_token() {
    // mock 返回两个 200:from_config 首次 + handle_401 re-login
    let (addr, _) = spawn_mock_server(vec![
        (200, r#"{"access_token":"initial"}"#.to_string()),
        (200, r#"{"access_token":"refreshed"}"#.to_string()),
    ])
    .await;
    let cfg = login_cfg(format!("http://{addr}/login"), "access_token");
    let s = Session::from_config(cfg, None).await.expect("from_config");
    assert_eq!(
        s.headers().await.get("Authorization").map(String::as_str),
        Some("Bearer initial")
    );
    // 触发 re-login,headers 更新
    assert!(s.handle_401().await.unwrap());
    let h = s.headers().await;
    assert_eq!(
        h.get("Authorization").map(String::as_str),
        Some("Bearer refreshed")
    );
}

#[tokio::test]
async fn loop_guard_blocks_second_401_within_1s() {
    let s = Session::from_cli_flags(&["abc".to_string()], &[]).unwrap();
    s.handle_401().await.unwrap(); // 静态 → false
    s.handle_401().await.unwrap(); // loop guard 拦截 → false（不调 network）
}

#[tokio::test]
async fn empty_session_used_by_caller() {
    let s = Session::empty();
    let _caller = HttpCaller::new(s).expect("caller");
}
