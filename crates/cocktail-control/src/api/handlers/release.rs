use std::sync::OnceLock;

use axum::Json;
use axum::extract::Query;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::release_check::{
    self, PlatformPick, ReleaseCache, ReleaseInfo, RepoConfig, UpdateStatus,
};

#[derive(Deserialize)]
pub struct UpdateQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Serialize)]
pub struct UpdateResponse {
    pub status: UpdateStatus,
    pub channels: std::collections::BTreeMap<String, usize>,
    pub releases: Vec<ReleaseRow>,
}

#[derive(Serialize)]
pub struct ReleaseRow {
    pub tag: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub html_url: String,
    pub notes: String,
}

#[derive(Serialize)]
pub struct AssetsResponse {
    pub tag: Option<String>,
    pub picks: Vec<PlatformPick>,
    pub error: Option<String>,
}

fn cache() -> &'static ReleaseCache {
    static C: OnceLock<ReleaseCache> = OnceLock::new();
    C.get_or_init(ReleaseCache::new)
}

fn repo() -> &'static RepoConfig {
    static R: OnceLock<RepoConfig> = OnceLock::new();
    R.get_or_init(RepoConfig::default)
}

async fn fetch_releases(force: bool) -> anyhow::Result<Vec<ReleaseInfo>> {
    let cfg = repo();
    if !force {
        if let Some(list) = cache().fresh(cfg.ttl) {
            return Ok(list);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("cocktail-control")
        .build()?;
    let resp = client
        .get(cfg.api_url())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("发布接口返回 {}", resp.status());
    }
    let text = resp.text().await?;
    let list =
        release_check::parse_releases(&text).ok_or_else(|| anyhow::anyhow!("发布清单解析失败"))?;
    cache().store(list.clone());
    Ok(list)
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub async fn update_status(Query(q): Query<UpdateQuery>) -> Json<UpdateResponse> {
    let channel = q.channel.unwrap_or_else(|| "beta".into());
    let force = q.force.unwrap_or(false);
    match fetch_releases(force).await {
        Ok(list) => {
            let filtered = release_check::channel_filter(&list, &channel);
            let status = release_check::evaluate(
                &current_version(),
                &filtered,
                channel != "stable",
                "github",
            );
            let releases = filtered
                .iter()
                .take(10)
                .map(|r| ReleaseRow {
                    tag: r.tag.clone(),
                    name: r.name.clone(),
                    published_at: r.published_at.clone(),
                    prerelease: r.prerelease,
                    html_url: r.html_url.clone(),
                    notes: r.body.chars().take(500).collect(),
                })
                .collect();
            Json(UpdateResponse {
                status,
                channels: release_check::summarize_channels(&list),
                releases,
            })
        }
        Err(e) => Json(UpdateResponse {
            status: UpdateStatus {
                current: current_version(),
                latest: None,
                update_available: false,
                latest_is_stable: false,
                checked_at: chrono::Utc::now().to_rfc3339(),
                source: "github".into(),
                notes_url: repo().page_url(),
                error: Some(e.to_string()),
            },
            channels: Default::default(),
            releases: Vec::new(),
        }),
    }
}

pub async fn update_assets() -> (StatusCode, Json<AssetsResponse>) {
    let platforms = [
        "linux-x64-deb",
        "linux-x64-rpm",
        "linux-arm64-deb",
        "windows-x64-zip",
        "windows-x64-msi",
    ];
    match fetch_releases(false).await {
        Ok(list) => {
            let picked =
                release_check::newest(&list, false).or_else(|| release_check::newest(&list, true));
            match picked {
                Some((rel, _)) => {
                    let picks = platforms
                        .iter()
                        .map(|p| release_check::pick_asset(rel, p))
                        .collect();
                    (
                        StatusCode::OK,
                        Json(AssetsResponse {
                            tag: Some(rel.tag.clone()),
                            picks,
                            error: None,
                        }),
                    )
                }
                None => (
                    StatusCode::OK,
                    Json(AssetsResponse {
                        tag: None,
                        picks: Vec::new(),
                        error: Some("没有可用发布".into()),
                    }),
                ),
            }
        }
        Err(e) => (
            StatusCode::OK,
            Json(AssetsResponse {
                tag: None,
                picks: Vec::new(),
                error: Some(e.to_string()),
            }),
        ),
    }
}
