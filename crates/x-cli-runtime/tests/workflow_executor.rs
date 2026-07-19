//! WorkflowExecutor 端到端测试
//!
//! 用本地 mock HTTP server 验证（无外部网络依赖）：
//! - InputRef 三种（$input / $steps / Static）解析
//! - 顺序执行 + 上一步响应填下步
//! - workflow.run JSON-RPC 协议

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use x_cli_core::ir::{ApiSpec, Workflow};
use x_cli_core::parse_openapi_str;
use x_cli_core::parse_workflow_str;
use x_cli_runtime::{serve, AuthProfile, HttpCaller};

const HTTPBIN: &str = include_str!("fixtures/httpbin.yaml");

fn spec_with_base(base: &str) -> Arc<ApiSpec> {
    let mut s = parse_openapi_str(HTTPBIN).expect("parse httpbin");
    s.base_url = Some(base.to_string());
    Arc::new(s)
}

fn workflow(yaml: &str) -> Arc<Workflow> {
    Arc::new(parse_workflow_str(yaml).expect("parse workflow"))
}

/// 启动本地 HTTP server，每个请求返回固定 JSON body
/// 返回 (server base url, JoinHandle)
async fn spawn_local_server(response_body: String) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    let url = format!("http://{}", addr);
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            let body = response_body.clone();
            tokio::spawn(async move {
                // 简单 HTTP/1.1：读 request line（不严格解析），返回固定 body
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, handle)
}

/// 构造一个回显 body 的 mock server：把请求 path 写进 body.url
async fn spawn_echo_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    let url = format!("http://{}", addr);
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = match socket.read(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => return,
                };
                let req = String::from_utf8_lossy(&buf[..n]);
                // 提取第一行（"GET /path HTTP/1.1"）的 path
                let path = req
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let body = serde_json::json!({
                    "args": {},
                    "data": "",
                    "files": {},
                    "form": {},
                    "headers": {},
                    "json": null,
                    "method": "GET",
                    "origin": "127.0.0.1",
                    "url": format!("http://{}{}", addr, path),
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, handle)
}

/// 启动 serve + 发一个 JSON-RPC 请求 + 收响应
async fn run_rpc(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    request: &str,
) -> Vec<String> {
    let caller = HttpCaller::new(AuthProfile::default()).expect("caller");
    serve_then(spec, workflows, request, caller).await
}

/// 自定义 caller（用于测试 auth header 注入）
async fn serve_then(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    request: &str,
    caller: HttpCaller,
) -> Vec<String> {
    let (mut client_write, server_read) = duplex(4096);
    let (server_write, mut client_read) = duplex(4096);

    let base_url = spec.base_url.clone();

    let serve_task = tokio::spawn(async move {
        serve(server_read, server_write, spec, workflows, base_url, caller).await;
    });

    client_write.write_all(request.as_bytes()).await.unwrap();
    client_write.write_all(b"\n").await.unwrap();
    drop(client_write);

    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client_read.read_to_end(&mut buf),
    )
    .await
    .expect("response timed out (10s)");
    serve_task.await.unwrap();

    let s = String::from_utf8(buf).expect("utf8");
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[tokio::test]
async fn workflow_run_two_steps_chains_response_body() {
    // 用本地 echo server（无网络依赖）
    let (base, _handle) = spawn_echo_server().await;
    let wf = workflow(
        r#"
name: chain-demo
description: 验证 step 之间 InputRef 解析
steps:
  - name: step1
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "from-step1"
  - name: step2
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "$steps.step1.response.body.url"
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );
    let resp = run_rpc(spec_with_base(&base), wfs, &req).await;
    assert_eq!(resp.len(), 1, "expected 1 response, got: {resp:?}");

    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse resp");
    let result = v.get("result").expect("result field");
    assert_eq!(result.get("status").and_then(|s| s.as_str()), Some("ok"));
    let steps = result.get("steps").and_then(|s| s.as_array()).expect("steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].get("status").and_then(|s| s.as_u64()), Some(200));
    assert_eq!(steps[1].get("status").and_then(|s| s.as_u64()), Some(200));
    let step2_url = steps[1]
        .get("body")
        .and_then(|b| b.get("url"))
        .and_then(|u| u.as_str())
        .expect("step2.body.url");
    assert!(
        step2_url.contains("from-step1"),
        "step2.url should contain 'from-step1' (chained from step1's path), got: {step2_url}"
    );
    let outputs = result.get("outputs").expect("outputs");
    assert!(outputs.get("url").is_some());
}

#[tokio::test]
async fn workflow_run_uses_external_input() {
    let (base, _handle) = spawn_echo_server().await;
    let wf = workflow(
        r#"
name: input-demo
steps:
  - name: only
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "$input.target"
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{"target":"hello-world"}}}}}}"#,
        wf.name
    );
    let resp = run_rpc(spec_with_base(&base), wfs, &req).await;
    assert_eq!(resp.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let result = v.get("result").expect("result");
    let outputs = result.get("outputs").expect("outputs");
    let url = outputs.get("url").and_then(|u| u.as_str()).expect("url");
    assert!(
        url.contains("hello-world"),
        "url should contain 'hello-world' from $input.target, got: {url}"
    );
}

#[tokio::test]
async fn workflow_run_unknown_workflow_returns_error() {
    let (base, _handle) = spawn_echo_server().await;
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{"workflow":"nonexistent","inputs":{}}}"#;
    let resp = run_rpc(spec_with_base(&base), BTreeMap::new(), req).await;
    assert_eq!(resp.len(), 1);
    assert!(
        resp[0].contains("\"code\":-32010"),
        "expected WORKFLOW_NOT_FOUND (-32010), got: {}",
        resp[0]
    );
    assert!(resp[0].contains("nonexistent"));
}

#[tokio::test]
async fn workflow_run_missing_external_input_returns_error() {
    let (base, _handle) = spawn_echo_server().await;
    let wf = workflow(
        r#"
name: needs-input
steps:
  - name: only
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "$input.required_field"
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );
    let resp = run_rpc(spec_with_base(&base), wfs, &req).await;
    assert_eq!(resp.len(), 1);
    assert!(
        resp[0].contains("\"code\":-32012"),
        "expected WORKFLOW_INPUT_INVALID (-32012), got: {}",
        resp[0]
    );
    assert!(resp[0].contains("required_field"));
}

// ─────────────── F 阶段：DAG 拓扑执行 ───────────────

#[tokio::test]
async fn workflow_runs_in_topological_order_when_depends_on_specified() {
    // 数组顺序：read → create（"违反" 依赖）
    // depends_on：create → read
    // 拓扑序应该是 create → read
    let (base, _handle) = spawn_echo_server().await;
    let wf = workflow(
        r#"
name: topo-demo
steps:
  - name: read
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "second"
    depends_on: [create]
  - name: create
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "first"
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );
    let resp = run_rpc(spec_with_base(&base), wfs, &req).await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let result = v.get("result").expect("result");
    let steps = result.get("steps").and_then(|s| s.as_array()).expect("steps");

    // 拓扑序：create (无依赖) 先，read 后
    assert_eq!(steps[0].get("name").and_then(|n| n.as_str()), Some("create"));
    assert_eq!(steps[1].get("name").and_then(|n| n.as_str()), Some("read"));

    // 第一个 step 的 url 含 "first"
    let first_url = steps[0]
        .get("body")
        .and_then(|b| b.get("url"))
        .and_then(|u| u.as_str())
        .expect("url");
    assert!(first_url.contains("first"), "first step should be `create` (path=first), got: {first_url}");
}

#[tokio::test]
async fn workflow_runs_in_array_order_when_no_depends_on() {
    // 没有 depends_on → 数组顺序（向后兼容）
    let (base, _handle) = spawn_echo_server().await;
    let wf = workflow(
        r#"
name: array-order
steps:
  - name: first
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "1"
  - name: second
    endpoint: echo__get__anything_path
    inputs:
      path_params:
        path: "2"
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );
    let resp = run_rpc(spec_with_base(&base), wfs, &req).await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let result = v.get("result").expect("result");
    let steps = result.get("steps").and_then(|s| s.as_array()).expect("steps");
    assert_eq!(steps[0].get("name").and_then(|n| n.as_str()), Some("first"));
    assert_eq!(steps[1].get("name").and_then(|n| n.as_str()), Some("second"));
}

// ─────────────── H 阶段：认证 header 注入 ───────────────

use x_cli_runtime::build_auth_profile;

#[tokio::test]
async fn auth_bearer_token_is_injected_in_http_requests() {
    // spawn_auth_server：要求 Authorization: Bearer expected-token，否则 401
    // body 包含回显的 auth header 值
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    let url = format!("http://{}", addr);
    let expected = "expected-token-xyz";

    let expected_clone = expected.to_string();
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            let expected = expected_clone.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = match socket.read(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => return,
                };
                let req = String::from_utf8_lossy(&buf[..n]);
                let auth_line = req
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("authorization:"))
                    .unwrap_or("");
                let status = if auth_line.contains(&expected) {
                    "200"
                } else {
                    "401"
                };
                let body = format!(
                    r#"{{"received_auth":"{}"}}"#,
                    auth_line.replace("Authorization: ", "").replace("authorization: ", "")
                );
                let resp = format!(
                    "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    let wf = workflow(
        r#"
name: auth-test
steps:
  - name: call
    endpoint: echo__get__anything_path
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );

    // 1. 没 token：期待 401（executor 视 4xx 为 step failed，返回 error）
    let auth = build_auth_profile(&[], &[]).expect("build");
    let caller = HttpCaller::new(auth).expect("caller");
    let resp = serve_then(
        spec_with_base(&url),
        wfs.clone(),
        &req,
        caller,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let error = v.get("error").expect("401 without token should produce error");
    let code = error.get("code").and_then(|c| c.as_i64()).expect("code");
    assert_eq!(code, -32011, "should be WORKFLOW_STEP_FAILED");
    let data_status = error
        .get("data")
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_u64())
        .expect("data.status");
    assert_eq!(data_status, 401, "server reported 401");

    // 2. 有正确 token：期待 200
    let auth = build_auth_profile(&[expected.to_string()], &[]).expect("build");
    let caller = HttpCaller::new(auth).expect("caller");
    let resp = serve_then(spec_with_base(&url), wfs, &req, caller).await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let steps = v
        .get("result")
        .and_then(|r| r.get("steps"))
        .and_then(|s| s.as_array())
        .expect("steps");
    let first_status = steps[0]
        .get("status")
        .and_then(|s| s.as_u64())
        .expect("status");
    assert_eq!(first_status, 200, "with bearer should get 200");
    // body 里有 auth 回显
    let body = steps[0].get("body").and_then(|b| b.get("received_auth")).and_then(|s| s.as_str()).expect("body.received_auth");
    assert!(body.contains(expected), "body should echo auth header: {body}");
}

#[tokio::test]
async fn wrong_bearer_token_returns_401() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    let url = format!("http://{}", addr);
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let body = r#"{"ok":true}"#;
                let resp = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    let wf = workflow(
        r#"
name: auth-test
steps:
  - name: call
    endpoint: echo__get__anything_path
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );

    let auth = build_auth_profile(&["wrong-token".to_string()], &[]).expect("build");
    let caller = HttpCaller::new(auth).expect("caller");
    let resp = serve_then(spec_with_base(&url), wfs, &req, caller).await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    // 4xx 视作 step 失败
    assert!(
        v.get("error").is_some(),
        "wrong token should produce error response"
    );
    let code = v.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_i64()).expect("error code");
    assert_eq!(code, -32011, "should be WORKFLOW_STEP_FAILED");
}

#[tokio::test]
async fn custom_header_is_sent() {
    // 验证 --auth-header "X-API-Key=xxx" 也注入
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    let url = format!("http://{}", addr);
    let api_key = "secret-key-123";
    let expected_header = format!("X-API-Key: {api_key}");
    let expected_clone = expected_header.clone();
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => break,
            };
            let expected = expected_clone.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                // HTTP header name case-insensitive: lowercase compare
                let req_lower = req.to_lowercase();
                let expected_lower = expected.to_lowercase();
                let body = if req_lower.contains(&expected_lower) {
                    r#"{"ok":true}"#
                } else {
                    r#"{"error":"missing key"}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    let wf = workflow(
        r#"
name: custom-header-test
steps:
  - name: call
    endpoint: echo__get__anything_path
"#,
    );
    let mut wfs = BTreeMap::new();
    wfs.insert(wf.name.clone(), wf.clone());

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{{"workflow":{:?},"inputs":{{}}}}}}"#,
        wf.name
    );

    let auth = build_auth_profile(
        &[],
        &[format!("X-API-Key={api_key}"), "X-Tenant=acme".to_string()],
    )
    .expect("build");
    let caller = HttpCaller::new(auth).expect("caller");
    let resp = serve_then(spec_with_base(&url), wfs, &req, caller).await;
    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse");
    let steps = v
        .get("result")
        .and_then(|r| r.get("steps"))
        .and_then(|s| s.as_array())
        .expect("steps");
    // body 是 server 回的 {"ok":true}
    let body = &steps[0].get("body");
    assert_eq!(
        body.and_then(|b| b.get("ok")).and_then(|o| o.as_bool()),
        Some(true),
        "custom header X-API-Key should be sent: {body:?}"
    );
}
