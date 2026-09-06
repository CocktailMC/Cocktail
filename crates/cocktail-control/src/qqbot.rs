//! QQ Bot OpenAPI v2: access token + proactive text messages.
//! Docs: https://bot.q.qq.com/wiki/develop/api-v2/

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

const TOKEN_URLS: &[&str] = &[
    "https://bots.qq.com/app/getAppAccessToken",
    "https://api.bot.qq.com/app/getAppAccessToken",
];

#[derive(Debug, Clone, Default)]
pub struct QqConfig {
    pub app_id: String,
    pub app_secret: String,
    pub group_openid: String,
    pub user_openid: String,
    pub sandbox: bool,
}

impl QqConfig {
    pub fn ready(&self) -> bool {
        !self.app_id.is_empty()
            && !self.app_secret.is_empty()
            && (!self.group_openid.is_empty() || !self.user_openid.is_empty())
    }
}

#[derive(Default)]
pub struct QqClient {
    token: Mutex<Option<(String, Instant)>>,
    seq: Mutex<u32>,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    #[serde(default)]
    expires_in: serde_json::Value,
}

fn expires_secs(v: &serde_json::Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(7200)
        .clamp(60, 7200)
}

impl QqClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn send_text(
        &self,
        http: &reqwest::Client,
        cfg: &QqConfig,
        content: &str,
    ) -> anyhow::Result<()> {
        if !cfg.ready() {
            anyhow::bail!("QQ 机器人未配置完整（需要 AppID、AppSecret，以及群或用户 openid）");
        }
        let token = self.access_token(http, cfg).await?;
        let text = truncate(content, 1500);
        let seq = {
            let mut g = self.seq.lock().await;
            *g = g.wrapping_add(1).max(1);
            *g
        };
        let body = json!({
            "content": text,
            "msg_type": 0,
            "msg_seq": seq,
        });
        let mut last_err = None;
        if !cfg.group_openid.is_empty() {
            match post_message(http, cfg, &token, "groups", &cfg.group_openid, &body).await {
                Ok(()) => {}
                Err(e) => last_err = Some(e),
            }
        }
        if !cfg.user_openid.is_empty() {
            match post_message(http, cfg, &token, "users", &cfg.user_openid, &body).await {
                Ok(()) => last_err = None,
                Err(e) => last_err = Some(e),
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    async fn access_token(
        &self,
        http: &reqwest::Client,
        cfg: &QqConfig,
    ) -> anyhow::Result<String> {
        if let Some((tok, exp)) = self.token.lock().await.as_ref() {
            if Instant::now() + Duration::from_secs(90) < *exp {
                return Ok(tok.clone());
            }
        }
        let payload = json!({
            "appId": cfg.app_id,
            "clientSecret": cfg.app_secret,
        });
        let mut last = anyhow::anyhow!("无法获取 QQ access_token");
        for url in TOKEN_URLS {
            match http
                .post(*url)
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let raw = resp.text().await.unwrap_or_default();
                    if !status.is_success() {
                        last = anyhow::anyhow!("token HTTP {status}: {raw}");
                        continue;
                    }
                    let parsed: TokenResp = match serde_json::from_str(&raw) {
                        Ok(v) => v,
                        Err(_) => {
                            last = anyhow::anyhow!("token 响应无法解析: {raw}");
                            continue;
                        }
                    };
                    if parsed.access_token.is_empty() {
                        last = anyhow::anyhow!("token 为空: {raw}");
                        continue;
                    }
                    let ttl = expires_secs(&parsed.expires_in);
                    *self.token.lock().await =
                        Some((parsed.access_token.clone(), Instant::now() + Duration::from_secs(ttl)));
                    return Ok(parsed.access_token);
                }
                Err(e) => last = e.into(),
            }
        }
        Err(last)
    }
}

async fn post_message(
    http: &reqwest::Client,
    cfg: &QqConfig,
    token: &str,
    kind: &str,
    openid: &str,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    let bases: Vec<&str> = if cfg.sandbox {
        vec![
            "https://sandbox.api.sgroup.qq.com",
            "https://api.bot.qq.com",
        ]
    } else {
        vec!["https://api.sgroup.qq.com", "https://api.bot.qq.com"]
    };
    let mut last = anyhow::anyhow!("发送失败");
    for base in bases {
        let url = format!("{base}/v2/{kind}/{openid}/messages");
        match http
            .post(&url)
            .header("Authorization", format!("QQBot {token}"))
            .header("X-Union-Appid", &cfg.app_id)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(body)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let raw = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    return Ok(());
                }
                last = anyhow::anyhow!("{status} {raw}");
                if status.as_u16() == 404 {
                    continue;
                }
                return Err(last);
            }
            Err(e) => last = e.into(),
        }
    }
    Err(last)
}

fn truncate(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
