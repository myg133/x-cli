//! 错误类型

use thiserror::Error;

/// `x-cli-core` 的顶层错误类型，涵盖解析、IO、序列化、校验等场景。
#[derive(Debug, Error)]
pub enum Error {
    /// OpenAPI 文档解析失败（格式错误、不支持的版本等）。
    #[error("OpenAPI 解析失败: {0}")]
    OpenApiParse(String),

    /// IO 错误（文件读写、网络等）。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// YAML 解析失败（serde_yaml 错误）。
    #[error("YAML 解析失败: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// JSON 解析失败（serde_json 错误）。
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),

    /// IR（中间表示）不合法（校验失败）。
    #[error("IR 不合法: {0}")]
    InvalidIr(String),

    /// 协议错误（JSON-RPC 请求/响应格式异常）。
    #[error("协议错误: {0}")]
    Protocol(String),
}

/// `x-cli-core` 的便捷类型别名。
pub type Result<T> = std::result::Result<T, Error>;
