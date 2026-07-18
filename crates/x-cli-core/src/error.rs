//! 错误类型

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("OpenAPI 解析失败: {0}")]
    OpenApiParse(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML 解析失败: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IR 不合法: {0}")]
    InvalidIr(String),

    #[error("协议错误: {0}")]
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;
