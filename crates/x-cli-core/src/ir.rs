//! IR 数据模型
//!
//! x-cli 的中间表示。emitter 把 IR 渲染成各平台 skill 描述，runtime 把 IR 实例化执行。
//! 这个模型是 OpenAPI 的"语义投影"——只保留对生成 skill 有用的信息。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 整个 API 文档的 IR。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSpec {
    /// 文档标题
    pub title: String,
    /// 文档版本
    pub version: String,
    /// 文档描述
    #[serde(default)]
    pub description: Option<String>,
    /// 默认 base URL（从 servers[0] 推断）
    #[serde(default)]
    pub base_url: Option<String>,
    /// 业务域（按 tag 归类）
    #[serde(default)]
    pub domains: Vec<Domain>,
    /// 全部接口（按 id 索引）
    pub endpoints: BTreeMap<String, Endpoint>,
}

/// 业务域
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    /// 域名称，如 `pet`、`user`。
    pub name: String,
    /// 域描述。
    #[serde(default)]
    pub description: Option<String>,
    /// 该域下全部接口的 id
    pub endpoint_ids: Vec<String>,
}

/// 单个 HTTP 接口
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// 稳定 id（格式 `<domain>.<method>.<sanitized_path>`），skill 引用全靠它
    pub id: String,
    /// 所属域
    pub domain: String,
    /// HTTP 方法
    pub method: HttpMethod,
    /// URL 路径
    pub path: String,
    /// OpenAPI operationId
    #[serde(default)]
    pub operation_id: Option<String>,
    /// 接口摘要
    #[serde(default)]
    pub summary: Option<String>,
    /// 接口详细描述
    #[serde(default)]
    pub description: Option<String>,
    /// 标签列表（用于分类）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 路径/查询/请求头参数
    #[serde(default)]
    pub params: Vec<Param>,
    /// 请求体
    #[serde(default)]
    pub request_body: Option<RequestBody>,
    /// 响应定义
    #[serde(default)]
    pub responses: Vec<Response>,
    /// 是否已废弃
    #[serde(default)]
    pub deprecated: bool,
}

/// HTTP 请求方法。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// GET 请求
    Get,
    /// POST 请求
    Post,
    /// PUT 请求
    Put,
    /// PATCH 请求
    Patch,
    /// DELETE 请求
    Delete,
    /// HEAD 请求
    Head,
    /// OPTIONS 请求
    Options,
}

/// 接口参数（路径/查询/请求头）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    /// 参数名
    pub name: String,
    /// 参数位置
    pub location: ParamLocation,
    /// 是否必填
    #[serde(default)]
    pub required: bool,
    /// 参数描述
    #[serde(default)]
    pub description: Option<String>,
    /// 参数 schema 定义
    pub schema: SchemaRef,
}

/// 参数位置（对应 OpenAPI parameter location）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParamLocation {
    /// 路径参数（URL 模板变量）
    Path,
    /// 查询参数（URL ? 后）
    Query,
    /// 请求头参数
    Header,
    /// Cookie 参数
    Cookie,
}

/// HTTP 请求体定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    /// 是否必填
    #[serde(default)]
    pub required: bool,
    /// 常见 application/json；多类型时取第一个
    pub content_type: String,
    /// 请求体 schema
    pub schema: SchemaRef,
}

/// HTTP 响应定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// HTTP 状态码
    pub status: u16,
    /// 响应描述
    #[serde(default)]
    pub description: Option<String>,
    /// 响应 content-type
    #[serde(default)]
    pub content_type: Option<String>,
    /// 响应体 schema（可为空）
    #[serde(default)]
    pub schema: Option<SchemaRef>,
}

/// Schema 引用
///
/// Schema 引用
///
/// - `name` / `description`：给人看的类型名
/// - `json_schema`：完整 JSON Schema 序列化结果（运行时校验/转换备用）
/// - `resolved`：解析 $ref 后的结构化树（B 阶段新增），用于 emitter 渲染和后续 LLM 理解
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRef {
    /// 类型名（如 `Pet`、`Order`）
    pub name: String,
    /// 类型描述
    #[serde(default)]
    pub description: Option<String>,
    /// 完整 JSON Schema 值
    pub json_schema: serde_json::Value,
    /// 解析 $ref 后的结构化树
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Box<ResolvedSchema>>,
}

impl SchemaRef {
    /// 简化构造：未知类型用 `any` 表达
    pub fn any() -> Self {
        Self {
            name: "any".to_string(),
            description: None,
            json_schema: serde_json::json!({}),
            resolved: None,
        }
    }
}

/// 解析后的结构化 schema
///
/// properties 和 required 表达 Object；items 表达 Array。
/// 循环引用通过 `recursive: true` 标记回填，不再深入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSchema {
    /// Schema 种类（Object / Array / Scalar / Any）
    pub kind: SchemaKind,
    /// Object: 属性定义
    #[serde(default)]
    pub properties: BTreeMap<String, SchemaRef>,
    /// Object: 必填字段
    #[serde(default)]
    pub required: Vec<String>,
    /// Array: 元素类型
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<SchemaRef>>,
    /// true 表示此处遇到了循环引用（schema 名字已经在解析路径上）
    #[serde(default)]
    pub recursive: bool,
}

/// Schema 种类。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SchemaKind {
    /// 对象类型（有 properties）
    Object,
    /// 数组类型（有 items）
    Array,
    /// 标量类型（string / number / boolean / integer）
    Scalar,
    /// 任意类型（无约束）
    Any,
}

// ─────────────── Workflow（C 阶段） ───────────────

/// 一个多步工作流。
///
/// 步骤按数组顺序执行（显式步骤序列，agent 自己跑）。
/// inputs 字段支持三种值：
/// - `"$input.xxx"`：引用工作流外部输入
/// - `"$steps.<name>.response.body.<path>"`：引用上一步响应
/// - 其他字符串：原样作为静态值
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// 工作流名称
    pub name: String,
    /// 工作流描述
    #[serde(default)]
    pub description: Option<String>,
    /// 外部输入参数列表
    #[serde(default)]
    pub inputs: Vec<WorkflowInput>,
    /// 执行步骤（按拓扑序或数组顺序）
    pub steps: Vec<WorkflowStep>,
}

/// 工作流的外部输入参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInput {
    /// 参数名
    pub name: String,
    /// 参数类型（如 `string`、`number`、`boolean`）
    pub r#type: String,
    /// 参数描述（提示 agent 应填入什么值）
    #[serde(default)]
    pub description: Option<String>,
    /// 默认值
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

/// 工作流的一个步骤。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// 步骤名称（在同一工作流内唯一）
    pub name: String,
    /// 步骤描述（提示 agent 此步骤的作用）
    #[serde(default)]
    pub description: Option<String>,
    /// endpoint id（来自 ApiSpec.endpoints）
    pub endpoint: String,
    /// 显式依赖：此 step 执行前必须先完成的 step 名字列表。
    /// 不写则按数组顺序隐式依赖前一个 step。
    /// 一旦有任何 step 写了 depends_on，所有 step 都按拓扑序执行。
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 步骤输入参数
    #[serde(default)]
    pub inputs: StepInputs,
}

/// 步骤的输入参数。所有 value 在 YAML 里都写成字符串，
/// 运行时按 `$input.` / `$steps.` 前缀判断是引用还是静态值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepInputs {
    /// 路径参数
    #[serde(default)]
    pub path_params: BTreeMap<String, String>,
    /// 查询参数
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    /// 请求头参数
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// 请求体参数
    #[serde(default)]
    pub body: BTreeMap<String, String>,
}

/// 一个 input 引用（解析后的形态）。
#[derive(Debug, Clone, PartialEq)]
pub enum InputRef {
    /// 引用工作流外部输入
    Input(String),
    /// 引用上一步响应
    StepOutput {
        /// 步骤名称
        step: String,
        /// 响应中的 JSON path（如 `"response.body.id"`）
        path: Vec<String>,
    },
    /// 静态值
    Static(String),
}

impl InputRef {
    /// 从 YAML 字符串解析
    pub fn parse(s: &str) -> Self {
        if let Some(rest) = s.strip_prefix("$input.") {
            return InputRef::Input(rest.to_string());
        }
        if let Some(rest) = s.strip_prefix("$steps.") {
            // 格式：$steps.<name>.response.body.<dotted.path>
            let mut parts = rest.split('.');
            let step = parts.next().unwrap_or("").to_string();
            let path: Vec<String> = parts.map(|s| s.to_string()).collect();
            return InputRef::StepOutput { step, path };
        }
        InputRef::Static(s.to_string())
    }

    /// 渲染为 markdown 描述
    pub fn describe(&self) -> String {
        match self {
            InputRef::Input(name) => format!("$input.{name}"),
            InputRef::StepOutput { step, path } => {
                format!("$steps.{}.{}", step, path.join("."))
            }
            InputRef::Static(v) => format!("`{v}`"),
        }
    }
}

// ─────────────── D 阶段：CLI 工具（MCP） ───────────────

/// 一组 CLI 工具的 IR。
///
/// 由 FDE 的 agent 分析 CLI 文档后按此 schema 写 YAML，x-cli 解析后供 emitter 和 runtime 使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSpec {
    /// CLI 工具列表
    pub tools: Vec<CliTool>,
}

/// 单个 CLI 工具定义。
///
/// 覆盖主流 CLI 的 80% 使用场景：子命令 + 位置参数 + --flag/-s + 布尔开关。
/// TUI（交互式）CLI 不在支持范围内。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliTool {
    /// 工具唯一名字（MCP tools/list 里的 name）
    pub name: String,
    /// 工具描述（MCP tools/list 里的 description）
    #[serde(default)]
    pub description: Option<String>,
    /// 可执行文件（如 `kubectl`、`docker`）
    pub command: String,
    /// 子命令路径（如 `["get", "pods"]`）
    #[serde(default)]
    pub subcommand: Vec<String>,
    /// 参数定义
    #[serde(default)]
    pub args: Vec<CliArg>,
    /// 输出格式（MCP runtime 用，决定如何解析 stdout）
    #[serde(default)]
    pub output: CliOutputType,
}

/// CLI 参数定义。
///
/// 每个参数可以是 --flag、-s（short）、或位置参数。
/// 通过 `flag` / `shorthand` / `position` 三个字段表达，互斥。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliArg {
    /// 参数名（用作 MCP inputSchema 里的属性名）
    pub name: String,
    /// 参数描述
    #[serde(default)]
    pub description: Option<String>,
    /// 长参数名（如 `--namespace`）。与 `position` 互斥。
    #[serde(default)]
    pub flag: Option<String>,
    /// 短参数名（如 `"-n"`）。仅与 `flag` 配合使用。
    #[serde(default)]
    pub shorthand: Option<String>,
    /// 位置参数序号（0-based）。与 `flag` 互斥。
    #[serde(default)]
    pub position: Option<u32>,
    /// 是否必填
    #[serde(default)]
    pub required: bool,
    /// 默认值
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// 参数 schema 类型（复用 SchemaRef 的 json_schema）
    #[serde(default = "SchemaRef::any")]
    pub schema: SchemaRef,
    /// 是否可重复（如 `-v -v -v`）
    #[serde(default)]
    pub repeatable: bool,
}

/// CLI 工具输出格式。
///
/// MCP runtime 根据此字段决定如何解析子进程 stdout。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CliOutputType {
    /// stdout 是 JSON 格式（自动解析后返回结构化 content）
    Json,
    /// stdout 是纯文本（作为 text content 原样返回）
    #[default]
    Text,
    /// stdout 是 YAML 格式
    Yaml,
    /// 无输出（只关心 exit code）
    None,
}
