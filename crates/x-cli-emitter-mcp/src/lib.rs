//! MCP（Model Context Protocol）skill emitter。
//!
//! 把 `ApiSpec` + `Workflow[]` + `CliSpec` 转成 MCP 格式的 skill 目录。
//!
//! # 输出
//!
//! ```text
//! <out>/
//! ├── mcp-tools.json       # MCP tool 定义（agent / 客户端加载用）
//! ├── mcp-server.json      # MCP 服务器连接配置
//! └── .x-cli/
//!     ├── ir.json          # ApiSpec（runtime 加载用）
//!     └── cli.json         # CliSpec（可选，runtime 加载用）
//! ```
//!
//! # 不变量
//!
//! - `mcp-tools.json` 里的 tool `name` 与 `Endpoint.id` / `CliTool.name` / `workflow.<name>` 一致
//! - `inputSchema` 格式是 [JSON Schema]（MCP 协议要求）
//! - `.x-cli/ir.json` 格式与其他 emitter 兼容（serve 共用）

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use x_cli_core::{ApiSpec, CliArg, CliSpec, Endpoint, HttpMethod, SchemaRef, Workflow};

/// MCP style tool 定义。
#[derive(serde::Serialize)]
pub struct McpToolsFile {
    pub tools: Vec<serde_json::Value>,
}

/// MCP 服务器连接配置。
#[derive(serde::Serialize, serde::Deserialize)]
pub struct McpServerFile {
    /// MCP 协议版本
    pub protocol_version: String,
    /// 服务器信息
    pub server_info: McpServerInfo,
    /// 能力声明
    pub capabilities: McpCapabilities,
    /// 启动命令
    pub command: String,
    /// 启动参数
    pub args: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct McpCapabilities {
    pub tools: McpToolsCapability,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct McpToolsCapability {
    pub list_changed: bool,
}

/// MCP emitter。
pub struct McpEmitter;

impl McpEmitter {
    /// 生成 MCP skill 目录。
    ///
    /// - `spec` — OpenAPI 解析后的 ApiSpec
    /// - `workflows` — 业务工作流列表
    /// - `cli_spec` — （可选）CLI 工具定义
    /// - `out_dir` — 输出目录
    pub fn emit_mcp(
        spec: &ApiSpec,
        workflows: &[Workflow],
        cli_spec: Option<&CliSpec>,
        out_dir: &Path,
    ) -> Result<()> {
        // 构建 MCP tools
        let mut tools = Vec::new();

        // 1. endpoints → MCP tools
        for ep in spec.endpoints.values() {
            tools.push(build_endpoint_tool(ep));
        }

        // 2. workflows → MCP tools
        for wf in workflows {
            tools.push(build_workflow_tool(wf));
        }

        // 3. CLI tools → MCP tools
        if let Some(cs) = cli_spec {
            for ct in &cs.tools {
                tools.push(build_cli_tool(ct));
            }
        }

        // 写 mcp-tools.json
        let mcp_tools = McpToolsFile { tools };
        let mcp_tools_json =
            serde_json::to_string_pretty(&mcp_tools).context("序列化 mcp-tools.json")?;
        std::fs::write(out_dir.join("mcp-tools.json"), &mcp_tools_json)
            .context("写 mcp-tools.json")?;

        // 写 mcp-server.json
        let mcp_server = McpServerFile {
            protocol_version: "2025-03-26".into(),
            server_info: McpServerInfo {
                name: "x-cli".into(),
                version: "0.1.0".into(),
            },
            capabilities: McpToolsCapability {
                list_changed: false,
            }
            .into(),
            command: "x".into(),
            args: vec!["serve".into(), "--mcp".into(), "--skill".into(), ".".into()],
        };
        let mcp_server_json =
            serde_json::to_string_pretty(&mcp_server).context("序列化 mcp-server.json")?;
        std::fs::write(out_dir.join("mcp-server.json"), &mcp_server_json)
            .context("写 mcp-server.json")?;

        // 写 .x-cli/ir.json（ApiSpec）
        let cache_dir = out_dir.join(".x-cli");
        std::fs::create_dir_all(&cache_dir).context("创建 .x-cli 目录")?;
        let ir_json = serde_json::to_string_pretty(spec).context("序列化 ir.json")?;
        std::fs::write(cache_dir.join("ir.json"), &ir_json).context("写 ir.json")?;

        // 写 .x-cli/cli.json（如果提供了 CliSpec）
        if let Some(cs) = cli_spec {
            let cli_json = serde_json::to_string_pretty(cs).context("序列化 cli.json")?;
            std::fs::write(cache_dir.join("cli.json"), &cli_json).context("写 cli.json")?;
        }

        Ok(())
    }
}

// ── 构建单个 MCP tool ──

/// 把 Endpoint 转成 MCP tool 定义。
fn build_endpoint_tool(ep: &Endpoint) -> serde_json::Value {
    let desc = build_endpoint_description(ep);
    let (properties, required) = build_endpoint_input_schema(ep);

    serde_json::json!({
        "name": ep.id,
        "description": desc,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

/// 构建 Endpoint 描述。
fn build_endpoint_description(ep: &Endpoint) -> String {
    let summary = ep.summary.as_deref().unwrap_or("");
    let description = ep.description.as_deref().unwrap_or("");
    let method_path = format!("{} {}", http_method_str(&ep.method), ep.path);
    if description.is_empty() {
        if summary.is_empty() {
            method_path
        } else {
            format!("{method_path} — {summary}")
        }
    } else {
        format!("{method_path} — {summary}\n{description}")
    }
}

/// 构建 Endpoint 的 inputSchema properties + required。
fn build_endpoint_input_schema(
    ep: &Endpoint,
) -> (serde_json::Map<String, serde_json::Value>, Vec<String>) {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    // path / query / header 参数
    for p in &ep.params {
        let param_schema = schema_ref_to_mcp_json(&p.schema);
        properties.insert(p.name.clone(), param_schema);
        if p.required {
            required.push(p.name.clone());
        }
    }

    // request body → 作为 `body` 参数
    if let Some(rb) = &ep.request_body {
        properties.insert(
            "body".into(),
            serde_json::json!({
                "type": "object",
                "description": rb.schema.name,
            }),
        );
        if rb.required {
            required.push("body".into());
        }
    }

    (properties, required)
}

/// 把 Workflow 转成 MCP tool 定义。
fn build_workflow_tool(wf: &Workflow) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
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

    let desc = wf.description.clone().unwrap_or_else(|| wf.name.clone());

    serde_json::json!({
        "name": format!("workflow.{}", wf.name),
        "description": desc,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

/// 把 CliTool 转成 MCP tool 定义。
fn build_cli_tool(ct: &x_cli_core::CliTool) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for arg in &ct.args {
        let arg_schema = schema_ref_to_mcp_json(&arg.schema);
        let flag_info = build_arg_flag_info(arg);
        let mut param_map = match arg_schema {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        param_map.insert("description".into(), flag_info.into());
        properties.insert(arg.name.clone(), serde_json::Value::Object(param_map));

        if arg.required {
            required.push(arg.name.clone());
        }
    }

    // 把 command + subcommand 编码进 description
    let cmd_line = if ct.subcommand.is_empty() {
        ct.command.clone()
    } else {
        format!("{} {}", ct.command, ct.subcommand.join(" "))
    };

    let desc = ct
        .description
        .clone()
        .unwrap_or_else(|| format!("CLI: {cmd_line}"));

    serde_json::json!({
        "name": ct.name.clone(),
        "description": format!("{desc}\n执行命令：`{cmd_line}`"),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

// ── 辅助函数 ──

/// 构建参数 flag/position 的描述信息。
fn build_arg_flag_info(arg: &CliArg) -> String {
    let mut desc = arg.description.clone().unwrap_or_default();
    if let Some(ref f) = arg.flag {
        desc.push_str(&format!("（参数形式：{f}"));
        if let Some(ref s) = arg.shorthand {
            desc.push_str(&format!(" / {s}"));
        }
        desc.push('）');
    } else if let Some(pos) = arg.position {
        desc.push_str(&format!("（位置参数 #{pos}）"));
    }
    desc
}

/// HttpMethod → 大写字符串。
fn http_method_str(m: &HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Head => "HEAD",
        HttpMethod::Options => "OPTIONS",
    }
}

/// SchemaRef → MCP JSON Schema 片断。
fn schema_ref_to_mcp_json(schema: &SchemaRef) -> serde_json::Value {
    // 优先用已经生成的 json_schema
    if !schema.json_schema.is_null() {
        return schema.json_schema.clone();
    }
    // 否则按 name 推断
    serde_json::json!({
        "type": schema_type_to_mcp(&schema.name)
    })
}

/// IR schema type name → MCP / JSON Schema type。
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

impl From<McpToolsCapability> for McpCapabilities {
    fn from(tools: McpToolsCapability) -> Self {
        McpCapabilities { tools }
    }
}

// ── 测试 ──

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use x_cli_core::parse_cli_spec_str;

    fn sample_api_spec() -> ApiSpec {
        let json_val = json!({
            "title": "Test API",
            "version": "1.0.0",
            "endpoints": {
                "test__get__pets": {
                    "id": "test__get__pets",
                    "method": "GET",
                    "path": "/pets",
                    "domain": "test",
                    "params": [],
                    "summary": "获取宠物列表",
                    "description": "返回所有宠物",
                    "operation_id": null,
                    "tags": [],
                    "deprecated": false
                },
                "test__post__pets": {
                    "id": "test__post__pets",
                    "method": "POST",
                    "path": "/pets",
                    "domain": "test",
                    "params": [
                        {
                            "name": "name",
                            "location": "query",
                            "required": true,
                            "schema": {
                                "name": "string",
                                "json_schema": { "type": "string" }
                            }
                        }
                    ],
                    "summary": "创建宠物",
                    "request_body": {
                        "required": true,
                        "content_type": "application/json",
                        "schema": { "name": "Pet", "json_schema": { "type": "object" } }
                    }
                }
            },
            "domains": [
                {
                    "name": "test",
                    "description": "测试域",
                    "endpoint_ids": ["test__get__pets", "test__post__pets"]
                }
            ]
        });
        serde_json::from_value(json_val).unwrap()
    }

    fn sample_workflows() -> Vec<Workflow> {
        vec![Workflow {
            name: "买宠物并查询订单".into(),
            description: Some("创建宠物然后查订单".into()),
            inputs: vec![x_cli_core::WorkflowInput {
                name: "petName".into(),
                description: Some("宠物名字".into()),
                r#type: "string".into(),
                default: None,
            }],
            steps: vec![],
        }]
    }

    fn sample_cli_spec() -> CliSpec {
        let yaml = r#"
tools:
  - name: kubectl_get_pods
    description: "列出指定命名空间的 Pod"
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
        parse_cli_spec_str(yaml).unwrap()
    }

    #[test]
    fn emit_mcp_produces_tools_file() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_api_spec();
        let workflows = sample_workflows();

        McpEmitter::emit_mcp(&spec, &workflows, None, dir.path()).unwrap();

        let tools_path = dir.path().join("mcp-tools.json");
        assert!(tools_path.exists(), "mcp-tools.json 应存在");

        let content = std::fs::read_to_string(&tools_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let tools = parsed["tools"].as_array().unwrap();
        // 2 endpoints + 1 workflow
        assert_eq!(tools.len(), 3, "应有 3 个 tool");
    }

    #[test]
    fn emit_mcp_includes_cli_tools() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_api_spec();
        let workflows = sample_workflows();
        let cli_spec = sample_cli_spec();

        McpEmitter::emit_mcp(&spec, &workflows, Some(&cli_spec), dir.path()).unwrap();

        let content = std::fs::read_to_string(dir.path().join("mcp-tools.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let tools = parsed["tools"].as_array().unwrap();
        // 2 endpoints + 1 workflow + 1 cli tool
        assert_eq!(tools.len(), 4, "含 CLI 工具应有 4 个 tool");

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"kubectl_get_pods"));
        assert!(names.contains(&"workflow.买宠物并查询订单"));
    }

    #[test]
    fn emit_mcp_produces_cache_and_config() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_api_spec();
        let workflows = sample_workflows();
        let cli_spec = sample_cli_spec();

        McpEmitter::emit_mcp(&spec, &workflows, Some(&cli_spec), dir.path()).unwrap();

        // .x-cli/ir.json
        let ir_path = dir.path().join(".x-cli").join("ir.json");
        assert!(ir_path.exists(), ".x-cli/ir.json 应存在");

        // .x-cli/cli.json
        let cli_path = dir.path().join(".x-cli").join("cli.json");
        assert!(cli_path.exists(), ".x-cli/cli.json 应存在");

        // mcp-server.json
        let server_path = dir.path().join("mcp-server.json");
        assert!(server_path.exists(), "mcp-server.json 应存在");

        let server: McpServerFile =
            serde_json::from_str(&std::fs::read_to_string(&server_path).unwrap()).unwrap();
        assert_eq!(server.protocol_version, "2025-03-26");
        assert_eq!(server.command, "x");
    }

    #[test]
    fn endpoint_tool_has_correct_structure() {
        let spec = sample_api_spec();
        let ep = spec.endpoints.get("test__post__pets").unwrap();
        let tool = build_endpoint_tool(ep);

        assert_eq!(tool["name"], "test__post__pets");
        assert!(tool["description"].as_str().unwrap().contains("创建宠物"));

        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("name"), "应有 name 参数");
        assert!(props.contains_key("body"), "应有 body 参数");

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("name")), "name 应必填");
        assert!(required.contains(&json!("body")), "body 应必填");
    }

    #[test]
    fn cli_tool_includes_command_in_description() {
        let cli_spec = sample_cli_spec();
        let ct = &cli_spec.tools[0];
        let tool = build_cli_tool(ct);

        assert_eq!(tool["name"], "kubectl_get_pods");
        let desc = tool["description"].as_str().unwrap();
        assert!(desc.contains("kubectl get pods"), "description 应包含命令");

        let schema = &tool["inputSchema"];
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("namespace"));
        let namespace = &props["namespace"];
        assert!(namespace["description"]
            .as_str()
            .unwrap()
            .contains("--namespace"));
    }

    #[test]
    fn workflow_tool_has_workflow_prefix() {
        let workflows = sample_workflows();
        let tool = build_workflow_tool(&workflows[0]);

        assert_eq!(tool["name"], "workflow.买宠物并查询订单");
        let schema = &tool["inputSchema"];
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("petName"));
    }

    #[test]
    fn no_cli_spec_skips_cli_file() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_api_spec();
        let workflows = sample_workflows();

        McpEmitter::emit_mcp(&spec, &workflows, None, dir.path()).unwrap();

        let cli_path = dir.path().join(".x-cli").join("cli.json");
        assert!(!cli_path.exists(), "没有 CliSpec 时不应产生 cli.json");
    }
}
