//! WorkflowExecutor 端到端测试
//!
//! 用 httpbin 真实后端验证：
//! - InputRef 三种（$input / $steps / Static）解析
//! - 顺序执行 + 上一步响应填下步
//! - workflow.run JSON-RPC 协议

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{duplex, AsyncWriteExt};
use x_cli_core::ir::{ApiSpec, Workflow};
use x_cli_core::parse_openapi_str;
use x_cli_core::parse_workflow_str;
use x_cli_runtime::{serve, AuthProfile, HttpCaller};

const HTTPBIN: &str = include_str!("fixtures/httpbin.yaml");

fn spec() -> Arc<ApiSpec> {
    Arc::new(parse_openapi_str(HTTPBIN).expect("parse httpbin"))
}

fn workflow(yaml: &str) -> Arc<Workflow> {
    Arc::new(parse_workflow_str(yaml).expect("parse workflow"))
}

/// 启动 serve + 发一个 JSON-RPC 请求 + 收响应
async fn run_rpc(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    request: &str,
) -> Vec<String> {
    let (mut client_write, server_read) = duplex(4096);
    let (server_write, mut client_read) = duplex(4096);

    // base_url：测试里默认用 spec.base_url
    let base_url = spec.base_url.clone();

    let caller = HttpCaller::new(AuthProfile::default()).expect("caller");
    let serve_task = tokio::spawn(async move {
        serve(server_read, server_write, spec, workflows, base_url, caller).await;
    });

    client_write.write_all(request.as_bytes()).await.unwrap();
    client_write.write_all(b"\n").await.unwrap();
    drop(client_write);

    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client_read.read_to_end(&mut buf),
    )
    .await
    .expect("response timed out");
    serve_task.await.unwrap();

    let s = String::from_utf8(buf).expect("utf8");
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[tokio::test]
async fn workflow_run_two_steps_chains_response_body() {
    // 工作流：
    //   step1: GET /anything/create → 拿到 body.url
    //   step2: GET /anything/$steps.step1.response.body.url → 用上一步结果
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
    let resp = run_rpc(spec(), wfs, &req).await;
    eprintln!("DEBUG two-step resp: {resp:?}");
    assert_eq!(resp.len(), 1, "expected 1 response, got: {resp:?}");

    let v: serde_json::Value = serde_json::from_str(&resp[0]).expect("parse resp");
    // 必须有 result
    let result = v.get("result").expect("result field");
    assert_eq!(result.get("status").and_then(|s| s.as_str()), Some("ok"));
    // 两步
    let steps = result.get("steps").and_then(|s| s.as_array()).expect("steps");
    assert_eq!(steps.len(), 2);
    // step1 status 200
    assert_eq!(
        steps[0].get("status").and_then(|s| s.as_u64()),
        Some(200)
    );
    // step2 status 200
    assert_eq!(
        steps[1].get("status").and_then(|s| s.as_u64()),
        Some(200)
    );
    // step2.body.url 应该等于 step1 调用的 url（说明 InputRef 解析正确）
    let step2_url = steps[1]
        .get("body")
        .and_then(|b| b.get("url"))
        .and_then(|u| u.as_str())
        .expect("step2.body.url");
    assert!(
        step2_url.contains("from-step1"),
        "step2.url should contain 'from-step1' (chained from step1's path), got: {step2_url}"
    );
    // outputs = 最后一步 body
    let outputs = result.get("outputs").expect("outputs");
    assert!(outputs.get("url").is_some());
}

#[tokio::test]
async fn workflow_run_uses_external_input() {
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
    let resp = run_rpc(spec(), wfs, &req).await;
    eprintln!("DEBUG resp: {resp:?}");
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
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{"workflow":"nonexistent","inputs":{}}}"#;
    let resp = run_rpc(spec(), BTreeMap::new(), req).await;
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
    let resp = run_rpc(spec(), wfs, &req).await;
    assert_eq!(resp.len(), 1);
    assert!(
        resp[0].contains("\"code\":-32012"),
        "expected WORKFLOW_INPUT_INVALID (-32012), got: {}",
        resp[0]
    );
    assert!(resp[0].contains("required_field"));
}
