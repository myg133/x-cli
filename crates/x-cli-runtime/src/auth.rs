//! Auth profile 构造
//!
//! 把 CLI 的 `--auth-bearer` / `--auth-header` 参数组装成 `AuthProfile`，
//! 注入到 HTTP 请求。

use crate::http::AuthProfile;
use anyhow::{Context, Result};

/// bearer tokens → `Authorization: Bearer <token>`
/// headers → `KEY: VALUE` 直接塞
/// 后传入的覆盖先传入的
pub fn build_auth_profile(bearer: &[String], headers: &[String]) -> Result<AuthProfile> {
    let mut profile = AuthProfile::default();
    for token in bearer {
        profile
            .headers
            .insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    for h in headers {
        let (k, v) = h
            .split_once('=')
            .with_context(|| format!("--auth-header `{h}` 格式应为 KEY=VALUE"))?;
        let k = k.trim().to_string();
        let v = v.trim().to_string();
        if k.is_empty() {
            anyhow::bail!("--auth-header key 不能为空: {h}");
        }
        profile.headers.insert(k, v);
    }
    Ok(profile)
}
