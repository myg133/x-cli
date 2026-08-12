//! MCP transport 的回归测试
//!
//! 用 tokio::io::duplex 模拟 stdio，验证 initialize / tools/list / tools/call。

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{duplex, AsyncWriteExt};
use x_cli_core::ir::{ApiSpec, CliSpec};
use x_cli_core::parse_openapi_str;
use x_cli_runtime::{serve_mcp, HttpCaller, Session};

const PETSTORE: &str = include_str!("fixtures/petstore.yaml");

fn spec() -> Arc<ApiSpec> {
    Arc::new(parse_openapi_str(PETSTORE).expect("parse petstore"))
}

/// MCP transport 的 round_trip helper。
async fn mcp_round_trip(
    spec: Arc<ApiSpec>,
    cli_spec: Option<Arc<CliSpec>>,
    requests: &[&str],
) -> Vec<String> {
    let (mut client_write, server_read) = duplex(4096);
    let (server_write, mut client_read) = duplex(4096);

    let caller = HttpCaller::new(Session::empty()).expect("caller");
    let serve_task = tokio::spawn(async move {
        serve_mcp(
            server_read,
            server_write,
            spec,
            BTreeMap::new(),
            cli_spec,
            None,
            caller,
        )
        .await;
    });

    for req in requests {
        client_write.write_all(req.as_bytes()).await.unwrap();
        client_write.write_all(b"\n").await.unwrap();
    }
    drop(client_write);

    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
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
async fn mcp_initialize_handshake() {
    let resp = mcp_round_trip(
        spec(),
        None,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.0.0"}}}"#],
    )
    .await;
    assert_eq!(resp.len(), 1);
    assert!(resp[0].contains("\"protocolVersion\":\"2025-03-26\""));
    assert!(resp[0].contains("\"serverInfo\""));
    assert!(resp[0].contains("\"tools\""));
}

#[tokio::test]
async fn mcp_tools_list() {
    let resp = mcp_round_trip(
        spec(),
        None,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    )
    .await;
    // 应返回 2 条响应：initialize + tools/list
    assert_eq!(resp.len(), 2);
    // tools/list 响应应在第 2 条
    let tools_resp = &resp[1];
    assert!(tools_resp.contains("\"tools\""));
    // petstore.yaml 有多个 endpoint，至少应包含 1 个 tool
    let parsed: serde_json::Value = serde_json::from_str(tools_resp).unwrap();
    let tools = parsed["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty(), "tools 不应为空");
    // 检查第一个 tool 有正确的结构
    let first = &tools[0];
    assert!(first["name"].as_str().unwrap().len() > 0);
    assert!(first["inputSchema"]["type"] == "object");
}

#[tokio::test]
async fn mcp_tools_call_unknown_returns_error() {
    let resp = mcp_round_trip(
        spec(),
        None,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nonexistent","arguments":{}}}"#,
        ],
    )
    .await;
    assert_eq!(resp.len(), 2);
    let call_resp = &resp[1];
    // 应该返回 error（endpoint not found）
    assert!(call_resp.contains("\"code\":"));
    // MCP 错误码: -32002 (Tool Execution Error)
    assert!(
        call_resp.contains("\"code\":-32002"),
        "got: {}",
        call_resp
    );
}

#[tokio::test]
async fn mcp_unknown_method_returns_method_not_found() {
    let resp = mcp_round_trip(
        spec(),
        None,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"unknown_method"}"#,
        ],
    )
    .await;
    assert_eq!(resp.len(), 2);
    let err_resp = &resp[1];
    assert!(err_resp.contains("\"code\":-32601")); // Method Not Found
    assert!(err_resp.contains("unknown_method"));
}

#[tokio::test]
async fn mcp_cli_tool_in_tools_list() {
    // 构造 CliSpec
    let cli_yaml = r#"
tools:
  - name: kubectl_get_pods
    description: "列出 Pod"
    command: kubectl
    subcommand: ["get", "pods"]
    args:
      - name: namespace
        flag: --namespace
        shorthand: "-n"
        required: true
        description: "命名空间"
        schema:
          name: string
          json_schema: {"type": "string"}
    output: json
"#;
    let cli_spec = Arc::new(
        x_cli_core::parse_cli_spec_str(cli_yaml).unwrap(),
    );

    let resp = mcp_round_trip(
        spec(),
        Some(cli_spec),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    )
    .await;
    assert_eq!(resp.len(), 2);
    let parsed: serde_json::Value = serde_json::from_str(&resp[1]).unwrap();
    let tools = parsed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"kubectl_get_pods"),
        "tools 应包含 kubectl_get_pods, 实际有: {names:?}"
    );
}

#[tokio::test]
async fn mcp_invalid_json_returns_parse_error() {
    let resp = mcp_round_trip(spec(), None, &["this is not json"]).await;
    assert_eq!(resp.len(), 1);
    // MCP 也使用 JSON-RPC 的 -32700 Parse Error
    assert!(resp[0].contains("\"code\":-32700"), "got: {}", resp[0]);
}

#[tokio::test]
async fn mcp_notification_gets_no_response() {
    let resp = mcp_round_trip(
        spec(),
        None,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            // notifications/initialized 是 notification（无 id），不应有响应
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    )
    .await;
    // 只应收到 initialize + tools/list 的响应，notification 无响应
    assert_eq!(resp.len(), 2);
}
