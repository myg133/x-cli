//! `auth.yaml` 配置 schema
//!
//! 业务 skill 目录里的 `auth.yaml` 是 session bootstrap 的声明式配置。
//! x-cli serve 启动时按此文件自动登录拿 token、注入 Bearer header、在 401 时 re-login。
//!
//! Scope 详见 `out/x-cli-meta-skill/scope.md`。本模块只关心 schema 形状 + 校验,
//! 真正的登录 / 401 retry 逻辑在 `x-cli-runtime`。

use serde::{Deserialize, Serialize};

/// auth.yaml 顶层配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct AuthConfig {
    /// schema 版本(目前固定为 1)
    pub version: u32,
    /// token 来源(bearer 或 login 二选一)
    pub token: TokenSource,
}

/// token 来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenSource {
    /// 静态 token(等价于 `--auth-bearer` flag)
    Bearer {
        /// token 字符串
        bearer: String,
    },
    /// 启动时自动登录拿 token
    Login {
        /// login 配置
        #[serde(flatten)]
        login: Box<LoginConfig>,
    },
}

/// login 配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct LoginConfig {
    /// login 请求
    pub request: LoginRequest,
    /// login 响应解析
    pub response: LoginResponse,
    /// refresh 策略
    #[serde(default)]
    pub refresh: RefreshConfig,
}

/// login HTTP 请求
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct LoginRequest {
    /// 完整 URL,缺省 = 用 skill 的 base-url
    #[serde(default)]
    pub url: Option<String>,
    /// HTTP method,缺省 POST
    #[serde(default = "default_method")]
    pub method: String,
    /// 额外 header(如 Content-Type)
    #[serde(default)]
    pub headers: serde_json::Map<String, serde_json::Value>,
    /// request body(JSON object)
    #[serde(default)]
    pub body: serde_json::Value,
}

/// login 响应解析
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct LoginResponse {
    /// token 在响应 JSON 里的字段路径(dotted 语法,缺省 "access_token")
    #[serde(default = "default_token_path")]
    pub token_path: String,
    /// 过期时间字段路径(可选,如 "expires_in")
    #[serde(default)]
    pub expires_in_path: Option<String>,
    /// refresh token 字段路径(可选,如 "refresh_token")
    #[serde(default)]
    pub refresh_token_path: Option<String>,
}

/// refresh 策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct RefreshConfig {
    /// 收到 401 是否自动 re-login + retry,缺省 true
    #[serde(default = "default_true")]
    pub on_401: bool,
    /// 主动 refresh(v0.1 暂不实现,留字段占位)
    #[serde(default)]
    pub proactive: bool,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            on_401: true,
            proactive: false,
        }
    }
}

fn default_method() -> String {
    "POST".to_string()
}

fn default_token_path() -> String {
    "access_token".to_string()
}

fn default_true() -> bool {
    true
}

/// 解析 `auth.yaml` 字符串为 `AuthConfig`
///
/// # Errors
///
/// - 缺失 `version` / `token` 字段
/// - `bearer` 为空字符串
/// - YAML 语法错
pub fn parse_auth_config_str(s: &str) -> Result<AuthConfig, AuthParseError> {
    let cfg: AuthConfig = serde_yaml::from_str(s).map_err(AuthParseError::Yaml)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// auth.yaml 解析/校验错误
#[derive(Debug, thiserror::Error)]
pub enum AuthParseError {
    /// YAML 语法错
    #[error("auth.yaml YAML 解析失败: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// schema 不合法
    #[error("auth.yaml 校验失败: {0}")]
    Invalid(String),
}

/// 业务校验(serde 之外的多余字段检查)
fn validate(cfg: &AuthConfig) -> Result<(), AuthParseError> {
    if cfg.version != 1 {
        return Err(AuthParseError::Invalid(format!(
            "version 必须是 1,实际是 {}",
            cfg.version
        )));
    }
    match &cfg.token {
        TokenSource::Bearer { bearer } => {
            if bearer.trim().is_empty() {
                return Err(AuthParseError::Invalid("bearer 不能为空字符串".to_string()));
            }
        }
        TokenSource::Login { login } => {
            if login.refresh.on_401 && login.refresh.proactive {
                return Err(AuthParseError::Invalid(
                    "refresh.on_401=true 时 proactive 不应同时为 true(v0.1 不实现 proactive)"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! 单元测试 —— 跟 `tests/auth.rs` 集成测试互补
    //! 单元测试聚焦 schema 形状 / 默认值 / tag 区分

    use super::*;

    #[test]
    fn defaults_apply() {
        let yaml = r#"
version: 1
token:
  kind: login
  request:
    body: { username: admin, password: admin }
  response: {}
"#;
        let cfg = parse_auth_config_str(yaml).expect("parse");
        match cfg.token {
            TokenSource::Login { login } => {
                assert_eq!(login.request.method, "POST");
                assert_eq!(login.response.token_path, "access_token");
                assert!(login.refresh.on_401);
                assert!(!login.refresh.proactive);
            }
            _ => panic!("expected Login"),
        }
    }

    #[test]
    fn reject_empty_bearer() {
        let yaml = r#"
version: 1
token:
  kind: bearer
  bearer: "   "
"#;
        let err = parse_auth_config_str(yaml).unwrap_err();
        assert!(matches!(err, AuthParseError::Invalid(_)), "got {err:?}");
    }

    #[test]
    fn reject_wrong_version() {
        let yaml = r#"
version: 2
token:
  kind: bearer
  bearer: abc
"#;
        let err = parse_auth_config_str(yaml).unwrap_err();
        assert!(matches!(err, AuthParseError::Invalid(_)), "got {err:?}");
    }
}
