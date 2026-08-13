//! JSON-RPC 2.0 协议 schema
//!
//! skill ↔ x-cli 之间的 ABI。版本演进不破坏这个 schema。
//! 当前仅暴露一个 method `call`，后面会扩 `list_endpoints` / `describe` 等。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// JSON-RPC 版本号（固定 `"2.0"`）
    pub jsonrpc: String,
    /// 请求 id，用于关联响应
    pub id: RpcId,
    /// 调用的方法名
    pub method: RpcMethod,
    /// 方法参数（JSON Value）
    #[serde(default)]
    pub params: Value,
}

impl RpcRequest {
    /// 快速构造一个 `call` 请求
    pub fn call(id: RpcId, params: CallParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: RpcMethod::Call,
            params: serde_json::to_value(params).expect("CallParams serializes"),
        }
    }
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// JSON-RPC 版本号
    pub jsonrpc: String,
    /// 对应请求的 id
    pub id: RpcId,
    /// 成功时的结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// 失败时的错误
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// 错误码（`protocol.rs::error_code` 常量）
    pub code: i32,
    /// 错误消息
    pub message: String,
    /// 附加错误数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 请求/响应 id，用于关联请求和响应。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RpcId {
    /// 数字 id
    Number(i64),
    /// 字符串 id
    String(String),
    /// 空 id（通知类请求）
    Null,
}

/// 当前支持的 methods
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RpcMethod {
    /// 调用一个 endpoint
    #[serde(rename = "call")]
    Call,
    /// 执行一个 workflow
    #[serde(rename = "workflow.run")]
    WorkflowRun,
    /// 健康检查
    #[serde(rename = "ping")]
    Ping,
}

/// `call` method 的参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallParams {
    /// 来自 IR 的 endpoint id
    pub endpoint_id: String,
    /// path 参数
    #[serde(default)]
    pub path_params: Value,
    /// query 参数
    #[serde(default)]
    pub query: Value,
    /// 额外请求头
    #[serde(default)]
    pub headers: Value,
    /// 请求体
    #[serde(default)]
    pub body: Option<Value>,
}

/// `call` method 的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallResult {
    /// HTTP 状态码
    pub status: u16,
    /// 响应头（JSON 对象）
    pub headers: Value,
    /// 响应体
    pub body: Value,
}

/// `workflow.run` method 的参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunParams {
    /// workflow 名字
    pub workflow: String,
    /// workflow 外部输入（按 name 取）
    #[serde(default)]
    pub inputs: Value,
}

/// `workflow.run` method 的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunResult {
    /// "ok" 或 "error"
    pub status: String,
    /// 每步的执行结果
    pub steps: Vec<WorkflowStepResult>,
    /// 最后一步响应 body（agent 通常拿这个）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Value>,
}

/// workflow 单步结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepResult {
    /// 步骤名称
    pub name: String,
    /// endpoint id
    pub endpoint: String,
    /// HTTP 状态码
    pub status: u16,
    /// 响应体
    pub body: Value,
}

/// 标准 JSON-RPC 错误码（JSON-RPC 2.0 规范 + x-cli 扩展）。
///
/// agent 端 hardcode 这些码，不可随意修改数值。
pub mod error_code {
    /// JSON-RPC 解析错误（-32700）
    pub const PARSE_ERROR: i32 = -32700;
    /// 无效请求（-32600）
    pub const INVALID_REQUEST: i32 = -32600;
    /// 找不到方法（-32601）
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// 参数无效（-32602）
    pub const INVALID_PARAMS: i32 = -32602;
    /// 内部错误（-32603）
    pub const INTERNAL_ERROR: i32 = -32603;
    // -32000 ~ -32099 服务端自定义
    /// 找不到 endpoint（-32001）
    pub const ENDPOINT_NOT_FOUND: i32 = -32001;
    /// HTTP 调用失败（-32002）
    pub const HTTP_ERROR: i32 = -32002;
    /// 认证失败（-32003）
    pub const AUTH_ERROR: i32 = -32003;
    /// 找不到 workflow（-32010）
    pub const WORKFLOW_NOT_FOUND: i32 = -32010;
    /// workflow 步骤执行失败（-32011）
    pub const WORKFLOW_STEP_FAILED: i32 = -32011;
    /// workflow 输入参数校验失败（-32012）
    pub const WORKFLOW_INPUT_INVALID: i32 = -32012;
}
