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
    pub name: String,
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
    pub method: HttpMethod,
    pub path: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub request_body: Option<RequestBody>,
    #[serde(default)]
    pub responses: Vec<Response>,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub location: ParamLocation,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    pub schema: SchemaRef,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParamLocation {
    Path,
    Query,
    Header,
    Cookie,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    #[serde(default)]
    pub required: bool,
    /// 常见 application/json；多类型时取第一个
    pub content_type: String,
    pub schema: SchemaRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub schema: Option<SchemaRef>,
}

/// Schema 引用
///
/// - `name` / `description`：给人看的类型名
/// - `json_schema`：完整 JSON Schema 序列化结果（运行时校验/转换备用）
/// - `resolved`：解析 $ref 后的结构化树（B 阶段新增），用于 emitter 渲染和后续 LLM 理解
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub json_schema: serde_json::Value,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SchemaKind {
    Object,
    Array,
    Scalar,
    Any,
}
