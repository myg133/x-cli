//! MCP（Model Context Protocol）skill emitter。
//!
//! 把 `ApiSpec` + `Workflow[]` + `CliSpec` 转成 MCP 格式的 skill 目录。
//!
//! # 设计原则
//!
//! MCP 对外只暴露**业务工具**（workflow），不直接暴露 HTTP endpoint。
//! 没有 workflow 的 endpoint 会自动生成一个**透传 workflow**（pass-through），
//! 让 agent 仍然可以调用，但感受不到底层 HTTP 细节。
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
//! - `mcp-tools.json` 里的 tool `name` 与 workflow 名称一致
//! - `inputSchema` 格式是 [JSON Schema]（MCP 协议要求）
//! - `.x-cli/ir.json` 格式与其他 emitter 兼容（serve 共用）

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use x_cli_core::{
    ApiSpec, CliArg, CliSpec, CliTool, Endpoint, StepInputs, Workflow, WorkflowInput, WorkflowStep,
};

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
        // 构建完整 workflows 映射：用户定义 + 自动生成透传
        let all_workflows = build_all_workflows(spec, workflows);

        // 构建 MCP tools
        let mut tools = Vec::new();

        // 1. workflows（用户定义 + 自动生成透传）→ MCP tools
        for wf in all_workflows.values() {
            tools.push(build_workflow_tool(wf));
        }

        // 2. CLI tools → MCP tools
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

// ── 构建完整 workflows 映射 ──

/// 构建完整的 workflows 映射：用户定义 + 自动生成透传。
fn build_all_workflows(spec: &ApiSpec, user_workflows: &[Workflow]) -> BTreeMap<String, Workflow> {
    let mut all = BTreeMap::new();

    // 用户定义的 workflow
    for wf in user_workflows {
        all.insert(wf.name.clone(), wf.clone());
    }

    // 检查每个 endpoint 是否有 workflow 覆盖
    for ep in spec.endpoints.values() {
        let ep_name = workflow_name_for_endpoint(ep);
        if all.contains_key(&ep_name) {
            continue;
        }
        // 自动生成透传 workflow
        let wf = auto_generate_workflow(ep);
        all.insert(ep_name, wf);
    }

    all
}

// ── 构建单个 MCP tool ──

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
        "name": wf.name,
        "description": desc,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

/// 把 CliTool 转成 MCP tool 定义。
fn build_cli_tool(ct: &CliTool) -> serde_json::Value {
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

    // 构建透传 step inputs
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

/// SchemaRef → MCP JSON Schema 片断。
fn schema_ref_to_mcp_json(schema: &x_cli_core::SchemaRef) -> serde_json::Value {
    if !schema.json_schema.is_null() {
        return schema.json_schema.clone();
    }
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
            inputs: vec![WorkflowInput {
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
        // 1 user workflow + 2 auto-generated (for 2 endpoints without workflow)
        assert_eq!(tools.len(), 3, "应有 3 个 tool（1 workflow + 2 透传）");
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"买宠物并查询订单"));
        assert!(names.contains(&"获取宠物列表"));
        assert!(names.contains(&"创建宠物"));
        // 不再直接暴露 endpoint id
        assert!(!names.contains(&"test__get__pets"));
        assert!(!names.contains(&"test__post__pets"));
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
        // 1 workflow + 2 auto-generated + 1 CLI tool
        assert_eq!(tools.len(), 4, "含 CLI 工具应有 4 个 tool");

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"kubectl_get_pods"));
        assert!(names.contains(&"买宠物并查询订单"));
    }

    #[test]
    fn emit_mcp_produces_cache_and_config() {
        let dir = tempfile::tempdir().unwrap();
        let spec = sample_api_spec();
        let workflows = sample_workflows();
        let cli_spec = sample_cli_spec();

        McpEmitter::emit_mcp(&spec, &workflows, Some(&cli_spec), dir.path()).unwrap();

        let ir_path = dir.path().join(".x-cli").join("ir.json");
        assert!(ir_path.exists(), ".x-cli/ir.json 应存在");

        let cli_path = dir.path().join(".x-cli").join("cli.json");
        assert!(cli_path.exists(), ".x-cli/cli.json 应存在");

        let server_path = dir.path().join("mcp-server.json");
        assert!(server_path.exists(), "mcp-server.json 应存在");

        let server: McpServerFile =
            serde_json::from_str(&std::fs::read_to_string(&server_path).unwrap()).unwrap();
        assert_eq!(server.protocol_version, "2025-03-26");
        assert_eq!(server.command, "x");
    }

    #[test]
    fn auto_generated_workflow_tools() {
        let spec = sample_api_spec();
        let workflows = sample_workflows();
        let all = build_all_workflows(&spec, &workflows);

        // 应有 3 个 workflow：1 用户定义 + 2 自动生成
        assert_eq!(all.len(), 3);

        // 用户定义的 workflow
        assert!(all.contains_key("买宠物并查询订单"));

        // 自动生成的透传 workflow（用 summary 命名）
        let get_pets = all.get("获取宠物列表").unwrap();
        assert!(get_pets
            .description
            .as_deref()
            .unwrap()
            .contains("GET /pets"));
        assert_eq!(get_pets.steps.len(), 1);
        assert_eq!(get_pets.steps[0].endpoint, "test__get__pets");

        let create_pet = all.get("创建宠物").unwrap();
        assert!(create_pet
            .description
            .as_deref()
            .unwrap()
            .contains("POST /pets"));
        assert_eq!(create_pet.steps.len(), 1);
        assert_eq!(create_pet.steps[0].endpoint, "test__post__pets");
        // 有 name 参数
        assert!(create_pet.inputs.iter().any(|i| i.name == "name"));
        // 有 body 参数
        assert!(create_pet.inputs.iter().any(|i| i.name == "body"));
    }

    #[test]
    fn workflow_tool_no_prefix() {
        let wf = Workflow {
            name: "买宠物并查询订单".into(),
            description: Some("创建宠物然后查订单".into()),
            inputs: vec![WorkflowInput {
                name: "petName".into(),
                description: Some("宠物名字".into()),
                r#type: "string".into(),
                default: None,
            }],
            steps: vec![],
        };
        let tool = build_workflow_tool(&wf);

        // 不再有 workflow. 前缀
        assert_eq!(tool["name"], "买宠物并查询订单");
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
