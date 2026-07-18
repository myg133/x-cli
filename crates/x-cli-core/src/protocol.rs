//! JSON-RPC 2.0 协议 schema
//!
//! skill ↔ x-cli 之间的 ABI。版本演进不破坏这个 schema。
//! 当前仅暴露一个 method `call`，后面会扩 `list_endpoints` / `describe` 等。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: RpcId,
    pub method: RpcMethod,
    #[serde(default)]
    pub params: Value,
}

impl RpcRequest {
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
    pub jsonrpc: String,
    pub id: RpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC id
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RpcId {
    Number(i64),
    String(String),
    Null,
}

/// 当前支持的 methods
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RpcMethod {
    /// 调用一个 endpoint
    Call,
    /// 健康检查
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
    pub status: u16,
    pub headers: Value,
    pub body: Value,
}

/// 标准 JSON-RPC 错误码
pub mod error_code {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    // -32000 ~ -32099 服务端自定义
    pub const ENDPOINT_NOT_FOUND: i32 = -32001;
    pub const HTTP_ERROR: i32 = -32002;
    pub const AUTH_ERROR: i32 = -32003;
}
