//! Session —— auth 状态 + login 能力
//!
//! Scope 详见 meta-skill `scope.md` L2。
//!
//! Session 负责:
//! - 持有当前 auth headers（`Authorization: Bearer xxx` 等）
//! - 收到 401 时按 `auth.yaml` 配置 re-login + 重试
//! - 防止无限重试（loop guard）
//!
//! Session 不负责:
//! - 把 token 写盘（重启 serve = 重登）
//! - 调度 cron / 主动 refresh（v0.2+）
//! - 多 session 隔离（一个 Session = 一个 token，多用户 = 多 Session 实例）

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use x_cli_core::{AuthConfig, LoginConfig, TokenSource};

/// 401 触发 re-login 后的最小冷却窗口（防止无限重试）
const LOOP_GUARD_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct SessionState {
    /// 当前要注入的 request headers
    headers: HashMap<String, String>,
    /// 401 时是否自动 re-login
    refresh_on_401: bool,
    /// login 配置（如 `None` = 静态 auth,不可 refresh）
    login_cfg: Option<LoginConfig>,
    /// 备用 base-url（`login_cfg.request.url` 为空时拼）
    base_url: Option<String>,
    /// 上次 401 时间（loop 防护）
    last_401_at: Option<Instant>,
}

/// Session 句柄（cheap clone via `Arc`）
#[derive(Clone, Debug)]
pub struct Session {
    inner: Arc<Mutex<SessionState>>,
    http: reqwest::Client,
}

impl Session {
    /// 从 `auth.yaml` 配置构造 + 立即登录（如 `kind=login`）
    ///
    /// # Errors
    ///
    /// - `kind=login` 时首次登录失败（网络 / 响应格式）
    /// - `bearer` 为空字符串
    pub async fn from_config(cfg: AuthConfig, base_url: Option<String>) -> Result<Self> {
        match cfg.token {
            TokenSource::Bearer { bearer } => Ok(Self::static_token(&bearer)),
            TokenSource::Login { login } => {
                let session = Self::from_login_config(*login, base_url);
                session.do_login().await?;
                Ok(session)
            }
        }
    }

    /// 从 CLI flag `--auth-bearer` + `--auth-header` 构造（静态,不可 refresh）
    pub fn from_cli_flags(bearer: &[String], headers: &[String]) -> Result<Self> {
        let mut h = HashMap::new();
        for token in bearer {
            h.insert("Authorization".to_string(), format!("Bearer {token}"));
        }
        for raw in headers {
            let (k, v) = raw
                .split_once('=')
                .with_context(|| format!("--auth-header `{raw}` 格式应为 KEY=VALUE"))?;
            let k = k.trim();
            let v = v.trim();
            if k.is_empty() {
                anyhow::bail!("--auth-header key 不能为空: {raw}");
            }
            h.insert(k.to_string(), v.to_string());
        }
        Ok(Self::wrap(h, false, None, None))
    }

    /// 空 session —— 无 auth,无 login。给测试和无 auth 后端用
    pub fn empty() -> Self {
        Self::wrap(HashMap::new(), false, None, None)
    }

    fn static_token(token: &str) -> Self {
        let mut h = HashMap::new();
        h.insert("Authorization".to_string(), format!("Bearer {token}"));
        Self::wrap(h, false, None, None)
    }

    fn from_login_config(login: LoginConfig, base_url: Option<String>) -> Self {
        let refresh = login.refresh.on_401;
        Self::wrap(HashMap::new(), refresh, Some(login), base_url)
    }

    fn wrap(
        headers: HashMap<String, String>,
        refresh_on_401: bool,
        login_cfg: Option<LoginConfig>,
        base_url: Option<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            inner: Arc::new(Mutex::new(SessionState {
                headers,
                refresh_on_401,
                login_cfg,
                base_url,
                last_401_at: None,
            })),
            http,
        }
    }

    /// 当前 headers 的快照
    pub async fn headers(&self) -> HashMap<String, String> {
        self.inner.lock().await.headers.clone()
    }

    /// 处理 401：按配置决定是否 re-login
    /// 返回 `true` 表示已成功 re-login + 更新 headers（调用方可以重试请求）
    pub async fn handle_401(&self) -> Result<bool> {
        // 取快照后释放锁,避免持锁做网络调用
        let (_refresh_on_401, login_cfg, base_url) = {
            let state = self.inner.lock().await;
            // loop 防护
            if let Some(t) = state.last_401_at {
                if t.elapsed() < LOOP_GUARD_WINDOW {
                    return Ok(false);
                }
            }
            if !state.refresh_on_401 || state.login_cfg.is_none() {
                let mut s = state;
                s.last_401_at = Some(Instant::now());
                return Ok(false);
            }
            (
                state.refresh_on_401,
                state.login_cfg.clone(),
                state.base_url.clone(),
            )
        };

        let login_cfg = login_cfg.context("login_cfg 应存在")?;
        let new_token = do_login(&self.http, &login_cfg, base_url.as_deref()).await?;

        let mut state = self.inner.lock().await;
        state
            .headers
            .insert("Authorization".to_string(), format!("Bearer {new_token}"));
        state.last_401_at = Some(Instant::now());
        Ok(true)
    }

    async fn do_login(&self) -> Result<()> {
        let (login_cfg, base_url) = {
            let state = self.inner.lock().await;
            let login = state.login_cfg.clone().context("Session 无 login 配置")?;
            (login, state.base_url.clone())
        };
        let token = do_login(&self.http, &login_cfg, base_url.as_deref()).await?;
        let mut state = self.inner.lock().await;
        state
            .headers
            .insert("Authorization".to_string(), format!("Bearer {token}"));
        Ok(())
    }
}

async fn do_login(
    http: &reqwest::Client,
    cfg: &LoginConfig,
    base_url: Option<&str>,
) -> Result<String> {
    let url = cfg
        .request
        .url
        .clone()
        .or_else(|| base_url.map(|b| format!("{}/login", b.trim_end_matches('/'))))
        .context("login.request.url 缺失且无 base-url 兜底")?;
    let method =
        reqwest::Method::from_bytes(cfg.request.method.as_bytes()).unwrap_or(reqwest::Method::POST);

    let mut req = http.request(method.clone(), &url);
    for (k, v) in &cfg.request.headers {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(&v.to_string()),
        ) {
            req = req.header(name, val);
        }
    }
    if !matches!(method, reqwest::Method::GET | reqwest::Method::HEAD) {
        req = req.json(&cfg.request.body);
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("login 请求失败: {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("login 端点返回 {status}: {body}"));
    }
    let body: serde_json::Value = resp.json().await.context("login 响应不是合法 JSON")?;

    extract_path(&body, &cfg.response.token_path)
        .map(|s| s.to_string())
        .with_context(|| format!("login 响应找不到 token 字段 `{}`", cfg.response.token_path))
}

fn extract_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_path_simple() {
        let v = serde_json::json!({ "access_token": "abc" });
        assert_eq!(extract_path(&v, "access_token"), Some("abc"));
    }

    #[test]
    fn extract_path_nested() {
        let v = serde_json::json!({ "data": { "access_token": "xyz" } });
        assert_eq!(extract_path(&v, "data.access_token"), Some("xyz"));
    }

    #[test]
    fn extract_path_missing_returns_none() {
        let v = serde_json::json!({ "data": {} });
        assert_eq!(extract_path(&v, "data.access_token"), None);
    }

    #[tokio::test]
    async fn from_cli_flags_empty() {
        let s = Session::from_cli_flags(&[], &[]).unwrap();
        assert!(s.headers().await.is_empty());
    }

    #[tokio::test]
    async fn from_cli_flags_bearer_sets_authorization() {
        let s = Session::from_cli_flags(&["abc".to_string()], &[]).unwrap();
        let h = s.headers().await;
        assert_eq!(
            h.get("Authorization").map(String::as_str),
            Some("Bearer abc")
        );
    }

    #[tokio::test]
    async fn from_cli_flags_malformed_header_rejected() {
        let err = Session::from_cli_flags(&[], &["NO-EQUALS".to_string()]).unwrap_err();
        assert!(err.to_string().contains("KEY=VALUE"));
    }

    #[tokio::test]
    async fn empty_session_handle_401_returns_false() {
        let s = Session::empty();
        assert!(!s.handle_401().await.unwrap());
    }

    #[tokio::test]
    async fn static_session_handle_401_returns_false() {
        let s = Session::from_cli_flags(&["abc".to_string()], &[]).unwrap();
        // 静态 token 没 login 配置,401 不能 refresh
        assert!(!s.handle_401().await.unwrap());
    }
}
