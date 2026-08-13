//! MCP（Model Context Protocol）stdio 传输层。
//!
//! 一行一个 JSON 请求，一行一个 JSON 响应（与现有 JSON-RPC 共享相同传输层）。
//!
//! 实现 MCP 协议方法：
//! - `initialize` — 握手
//! - `notifications/initialized` — 客户端通知（无响应）
//! - `tools/list` — 返回所有业务工具（workflow + CLI）
//! - `tools/call` — 执行 workflow 或 CLI 工具
//!
//! # 设计原则
//!
//! MCP 对外只暴露**业务工具**（workflow），不直接暴露 HTTP endpoint。
//! 没有 workflow 的 endpoint 会自动生成一个**透传 workflow**（pass-through），
//! 让 agent 仍然可以调用，但感受不到底层 HTTP 细节。

use crate::http::HttpCaller;
use crate::workflow_executor::WorkflowExecutor;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tracing::debug;
use x_cli_core::ir::{ApiSpec, CliSpec, Endpoint, Workflow, WorkflowInput, WorkflowStep, StepInputs};
use x_cli_core::protocol::{RpcError, RpcId, RpcResponse};

// ── MCP 协议常量 ──

const PROTOCOL_VERSION: &str = "2025-03-26";
const METHOD_INITIALIZE: &str = "initialize";
const METHOD_TOOLS_LIST: &str = "tools/list";
const METHOD_TOOLS_CALL: &str = "tools/call";
const NOTIF_INITIALIZED: &str = "notifications/initialized";

// ── 公开接口 ──

/// 启动 stdio 上的 MCP 服务。
pub async fn serve_mcp_stdio(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    cli_spec: Option<Arc<CliSpec>>,
    base_url: Option<String>,
    caller: HttpCaller,
) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    serve_mcp(stdin, stdout, spec, workflows, cli_spec, base_url, caller).await;
}

/// 在任意 reader/writer 上跑 MCP 服务（用于测试）。
pub async fn serve_mcp<R, W>(
    reader: R,
    mut writer: W,
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    cli_spec: Option<Arc<CliSpec>>,
    base_url: Option<String>,
    caller: HttpCaller,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // 构建完整 workflows 映射：用户定义 + 自动生成透传
    let all_workflows = build_all_workflows(&spec, &workflows);

    let mut lines = BufReader::new(reader).lines();
    let executor = Arc::new(WorkflowExecutor::new(
        spec.clone(),
        all_workflows.clone(),
        base_url.clone(),
        caller.clone(),
    ));

    let mut initialized = false;
    debug!("x-cli MCP transport ready");

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match handle_mcp_line(
            line,
            &spec,
            &all_workflows,
            cli_spec.as_deref(),
            &executor,
            &mut initialized,
        )
        .await
        {
            Ok(Some(resp)) => {
                if let Ok(json) = serde_json::to_string(&resp) {
                    let _ = writer.write_all(json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
            }
            Ok(None) => {
                // notification，不回复
            }
            Err(rpc_err) => {
                let id = parse_mcp_id(line);
                let resp = RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(rpc_err),
                };
                if let Ok(json) = serde_json::to_string(&resp) {
                    let _ = writer.write_all(json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
            }
        }
    }
    debug!("x-cli MCP transport exited (input closed)");
}

// ── MCP 方法处理 ──

async fn handle_mcp_line(
    line: &str,
    spec: &ApiSpec,
    all_workflows: &BTreeMap<String, Arc<Workflow>>,
    cli_spec: Option<&CliSpec>,
    executor: &Arc<WorkflowExecutor>,
    initialized: &mut bool,
) -> Result<Option<RpcResponse>, RpcError> {
    let req: Value = serde_json::from_str(line).map_err(|e| RpcError {
        code: -32700,
        message: format!("Parse error: {e}"),
        data: None,
    })?;

    let method = req["method"].as_str().unwrap_or("").to_string();
    let id = parse_mcp_id(line);

    match method.as_str() {
        METHOD_INITIALIZE => {
            *initialized = true;
            Ok(Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "x-cli",
                        "version": "0.1.0"
                    }
                })),
                error: None,
            }))
        }

        NOTIF_INITIALIZED => {
            *initialized = true;
            Ok(None)
        }

        METHOD_TOOLS_LIST => {
            let tools = build_mcp_tools(spec, all_workflows, cli_spec);
            Ok(Some(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({ "tools": tools })),
                error: None,
            }))
        }

        METHOD_TOOLS_CALL => {
            let params = req.get("params").unwrap_or(&Value::Null);
            let name = params["name"].as_str().unwrap_or("").to_string();
            let arguments = params.get("arguments").unwrap_or(&Value::Null);

            // 路由：workflow 走 executor，CLI 工具直接执行
            let result = if all_workflows.contains_key(&name) {
                handle_mcp_workflow_call(&name, arguments, executor).await
            } else if let Some(cs) = cli_spec {
                if cs.tools.iter().any(|t| t.name == name) {
                    handle_mcp_cli_call(&name, arguments, cli_spec)
                } else {
                    Err(format!("tool not found: {name}"))
                }
            } else {
                Err(format!("tool not found: {name}"))
            };

            match result {
                Ok(content) => Ok(Some(RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "content": content,
                        "isError": false,
                    })),
                    error: None,
                })),
                Err(e) => Err(RpcError {
                    code: -32002,
                    message: format!("Tool execution failed: {e}"),
                    data: None,
                }),
            }
        }

        _ => Err(RpcError {
            code: -32601,
            message: format!("Method not found: {method}"),
            data: None,
        }),
    }
}

// ── tool 执行 ──

/// 处理 workflow 工具调用（用户定义 + 自动生成透传）。
async fn handle_mcp_workflow_call(
    name: &str,
    arguments: &Value,
    executor: &Arc<WorkflowExecutor>,
) -> Result<Vec<Value>, String> {
    let inputs = match arguments {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };

    let result = executor
        .run(name, Value::Object(inputs))
        .await
        .map_err(|e| format!("{}: {}", e.code, e.message))?;

    Ok(vec![serde_json::json!({
        "type": "text",
        "text": serde_json::to_string_pretty(&result).unwrap_or_default(),
    })])
}

/// 处理 CLI 工具调用。
fn handle_mcp_cli_call(
    name: &str,
    arguments: &Value,
    cli_spec: Option<&CliSpec>,
) -> Result<Vec<Value>, String> {
    let cs = cli_spec.ok_or("no CLI spec loaded")?;
    let tool = cs
        .tools
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("CLI tool not found: {name}"))?;

    let args_map = match arguments {
        Value::Object(m) => m,
        _ => return Err("arguments must be an object".into()),
    };

    let mut cmd = std::process::Command::new(&tool.command);

    if !tool.subcommand.is_empty() {
        cmd.args(&tool.subcommand);
    }

    for arg in &tool.args {
        if let Some(val) = args_map.get(&arg.name) {
            if let Some(flag) = &arg.flag {
                cmd.arg(flag);
                if let Value::String(s) = val {
                    cmd.arg(s);
                } else if let Value::Number(n) = val {
                    cmd.arg(n.to_string());
                }
            }
        }
    }

    let mut positional: Vec<(u32, &str)> = tool
        .args
        .iter()
        .filter_map(|arg| {
            arg.position.and_then(|pos| {
                args_map
                    .get(&arg.name)
                    .map(|val| (pos, val.as_str().unwrap_or("")))
            })
        })
        .collect();
    positional.sort_by_key(|(pos, _)| *pos);
    for (_, val) in &positional {
        cmd.arg(val);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to execute CLI: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("CLI exited with {}: {stderr}", output.status));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(vec![serde_json::json!({
        "type": "text",
        "text": stdout,
    })])
}

// ── 构建 MCP tools 列表 ──

/// 构建完整的 workflows 映射：用户定义 + 自动生成透传。
fn build_all_workflows(
    spec: &ApiSpec,
    user_workflows: &BTreeMap<String, Arc<Workflow>>,
) -> BTreeMap<String, Arc<Workflow>> {
    let mut all = user_workflows.clone();

    for ep in spec.endpoints.values() {
        // 如果 endpoint 已经有 workflow（名称匹配），跳过
        let ep_name = workflow_name_for_endpoint(ep);
        if all.contains_key(&ep_name) {
            continue;
        }
        // 自动生成透传 workflow
        let wf = auto_generate_workflow(ep);
        all.insert(ep_name, Arc::new(wf));
    }

    all
}

/// 从 IR 构建 MCP tools 列表（用于 tools/list 响应）。
/// 只返回 workflow 和 CLI 工具，不直接暴露 endpoint。
fn build_mcp_tools(
    _spec: &ApiSpec,
    all_workflows: &BTreeMap<String, Arc<Workflow>>,
    cli_spec: Option<&CliSpec>,
) -> Vec<Value> {
    let mut tools = Vec::new();

    // workflows（用户定义 + 自动生成透传）
    for wf in all_workflows.values() {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for input in &wf.inputs {
            properties.insert(
                input.name.clone(),
                serde_json::json!({
                    "type": schema_type_to_mcp(&input.r#type),
                    "description": input.description.clone().unwrap_or_default(),
                }),
            );
            if input.default.is_none() {
                required.push(input.name.clone());
            }
        }
        tools.push(serde_json::json!({
            "name": wf.name,
            "description": wf.description.clone().unwrap_or_else(|| wf.name.clone()),
            "inputSchema": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        }));
    }

    // CLI tools
    if let Some(cs) = cli_spec {
        for ct in &cs.tools {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for arg in &ct.args {
                let arg_schema = arg.schema.json_schema.clone();
                let param_type = if !arg_schema.is_null() {
                    arg_schema["type"].as_str().unwrap_or("string").to_string()
                } else {
                    schema_type_to_mcp(&arg.schema.name).to_string()
                };

                let mut desc = arg.description.clone().unwrap_or_default();
                if let Some(ref flag) = arg.flag {
                    desc.push_str(&format!("（参数形式：{flag}"));
                    if let Some(ref sh) = arg.shorthand {
                        desc.push_str(&format!(" / {sh}"));
                    }
                    desc.push('）');
                } else if let Some(pos) = arg.position {
                    desc.push_str(&format!("（位置参数 #{pos}）"));
                }

                properties.insert(
                    arg.name.clone(),
                    serde_json::json!({
                        "type": param_type,
                        "description": desc,
                    }),
                );
                if arg.required {
                    required.push(arg.name.clone());
                }
            }

            let cmd_line = if ct.subcommand.is_empty() {
                ct.command.clone()
            } else {
                format!("{} {}", ct.command, ct.subcommand.join(" "))
            };
            let desc = ct
                .description
                .clone()
                .unwrap_or_else(|| format!("CLI 工具: {cmd_line}"));

            tools.push(serde_json::json!({
                "name": ct.name,
                "description": format!("{desc}\n执行命令：`{cmd_line}`"),
                "inputSchema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                }
            }));
        }
    }

    tools
}

// ── 自动生成透传 workflow ──

/// 为 endpoint 确定 workflow 名称。
fn workflow_name_for_endpoint(ep: &Endpoint) -> String {
    ep.summary
        .as_deref()
        .or(ep.operation_id.as_deref())
        .unwrap_or(&ep.id)
        .to_string()
}

/// 为没有 workflow 的 endpoint 自动生成透传 workflow。
/// 这样 MCP 对外暴露的全是 workflow，agent 无需直接操作 endpoint。
fn auto_generate_workflow(endpoint: &Endpoint) -> Workflow {
    let name = workflow_name_for_endpoint(endpoint);
    let description = format!("{} {}", http_method_str(&endpoint.method), endpoint.path);

    // 从 endpoint 参数生成 workflow inputs
    let mut inputs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in &endpoint.params {
        if seen.contains(&p.name) {
            continue;
        }
        seen.insert(p.name.clone());
        inputs.push(WorkflowInput {
            name: p.name.clone(),
            r#type: schema_type_to_mcp(&p.schema.name).to_string(),
            description: p.description.clone(),
            default: None,
        });
    }
    // 如果有 request body，也加一个 input
    if let Some(rb) = &endpoint.request_body {
        if !seen.contains("body") {
            seen.insert("body".to_string());
            inputs.push(WorkflowInput {
                name: "body".to_string(),
                r#type: "object".to_string(),
                description: Some(format!("请求体（{}）", rb.schema.name)),
                default: None,
            });
        }
    }

    // 构建透传 step inputs（所有参数用 $input.xxx 引用）
    let mut path_params = BTreeMap::new();
    let mut query = BTreeMap::new();
    let mut headers = BTreeMap::new();
    let mut body = BTreeMap::new();
    if endpoint.request_body.is_some() {
        body.insert("_raw".to_string(), "$input.body".to_string());
    }

    for p in &endpoint.params {
        let ref_val = format!("$input.{}", p.name);
        match p.location {
            x_cli_core::ParamLocation::Path => {
                path_params.insert(p.name.clone(), ref_val);
            }
            x_cli_core::ParamLocation::Query => {
                query.insert(p.name.clone(), ref_val);
            }
            x_cli_core::ParamLocation::Header => {
                headers.insert(p.name.clone(), ref_val);
            }
            x_cli_core::ParamLocation::Cookie => {
                headers.insert(p.name.clone(), ref_val);
            }
        }
    }

    let step = WorkflowStep {
        name: "call".to_string(),
        description: None,
        endpoint: endpoint.id.clone(),
        depends_on: vec![],
        inputs: StepInputs {
            path_params,
            query,
            headers,
            body,
        },
    };

    Workflow {
        name,
        description: Some(description),
        inputs,
        steps: vec![step],
    }
}

// ── helper 函数 ──

/// 从 arguments 中提取指定位置（path/query/header）的参数映射。
#[allow(dead_code)]
fn extract_params(
    arguments: &Value,
    params: &[x_cli_core::ir::Param],
    location: &str,
) -> Map<String, Value> {
    let mut result = Map::new();
    let args_map = match arguments {
        Value::Object(m) => m,
        _ => return result,
    };

    for p in params {
        let matches = match (&p.location, location) {
            (x_cli_core::ParamLocation::Path, "path") => true,
            (x_cli_core::ParamLocation::Query, "query") => true,
            (x_cli_core::ParamLocation::Header, "header") => true,
            _ => false,
        };
        if matches {
            if let Some(val) = args_map.get(&p.name) {
                result.insert(p.name.clone(), val.clone());
            }
        }
    }
    result
}

/// HttpMethod → 大写字符串。
fn http_method_str(m: &x_cli_core::HttpMethod) -> &'static str {
    match m {
        x_cli_core::HttpMethod::Get => "GET",
        x_cli_core::HttpMethod::Post => "POST",
        x_cli_core::HttpMethod::Put => "PUT",
        x_cli_core::HttpMethod::Patch => "PATCH",
        x_cli_core::HttpMethod::Delete => "DELETE",
        x_cli_core::HttpMethod::Head => "HEAD",
        x_cli_core::HttpMethod::Options => "OPTIONS",
    }
}

/// IR schema type → MCP JSON Schema type。
fn schema_type_to_mcp(name: &str) -> &str {
    match name.to_lowercase().as_str() {
        "string" | "str" => "string",
        "integer" | "int" | "long" => "integer",
        "number" | "float" | "double" | "decimal" => "number",
        "boolean" | "bool" => "boolean",
        "array" | "list" | "set" | "vector" => "array",
        "object" | "map" | "dict" | "any" => "object",
        _ => "string",
    }
}

/// 从 JSON 行中解析 id。
fn parse_mcp_id(line: &str) -> RpcId {
    if let Ok(v) = serde_json::from_str::<Value>(line) {
        if let Some(id) = v.get("id") {
            if id.is_null() {
                return RpcId::Null;
            }
            if let Some(n) = id.as_i64() {
                return RpcId::Number(n);
            }
            if let Some(s) = id.as_str() {
                return RpcId::String(s.to_string());
            }
        }
    }
    RpcId::Null
}