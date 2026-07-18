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
/// `name` 是给人看的类型名（用于生成 skill 描述），`json_schema` 是原始 JSON Schema
/// 用于运行时校验和参数解析。B 阶段可在此处加更多结构化字段（required、properties 等）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub json_schema: serde_json::Value,
}

impl SchemaRef {
    /// 简化构造：未知类型用 `any` 表达
    pub fn any() -> Self {
        Self {
            name: "any".to_string(),
            description: None,
            json_schema: serde_json::json!({}),
        }
    }
}
