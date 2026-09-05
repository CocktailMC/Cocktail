//! Hangar API client — https://hangar.papermc.io/api-docs
//! Paper / Velocity / Waterfall plugin repository (HangarMC/Hangar).

use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://hangar.papermc.io/api/v1";
const USER_AGENT: &str =
    "Cocktail-Manager/0.1 (contact=dev@local; +https://github.com/HangarMC/Hangar)";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("http client")
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    /// PAPER | VELOCITY | WATERFALL
    #[serde(default = "default_platform")]
    pub platform: String,
}

fn default_limit() -> u32 {
    20
}

fn default_platform() -> String {
    "PAPER".into()
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub slug: String,
    pub owner: String,
    pub name: String,
    pub description: String,
    pub downloads: i64,
    pub avatar_url: Option<String>,
    pub category: String,
    pub platforms: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Deserialize)]
pub struct VersionsQuery {
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Serialize, Clone)]
pub struct VersionInfo {
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub downloads: i64,
    pub filename: String,
    pub download_url: String,
    pub size: u64,
    pub game_versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub slug: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_platform")]
    pub platform: String,
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub path: String,
    pub filename: String,
    pub size: u64,
    pub slug: String,
    pub version: String,
    pub platform: String,
}

pub async fn search(q: &SearchQuery) -> anyhow::Result<SearchResponse> {
    let limit = q.limit.clamp(1, 50);
    let mut url = reqwest::Url::parse(&format!("{API}/projects"))?;
    {
        let mut qp = url.query_pairs_mut();
        if !q.query.is_empty() {
            qp.append_pair("q", &q.query);
        }
        qp.append_pair("limit", &limit.to_string());
        qp.append_pair("offset", &q.offset.to_string());
        qp.append_pair("sort", "-downloads");
        if !q.platform.is_empty() {
            qp.append_pair("platform", &q.platform.to_ascii_uppercase());
        }
    }

    let v: Value = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let pagination = v.get("pagination").cloned().unwrap_or(Value::Null);
    let hits: Vec<SearchHit> = v
        .get("result")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let ns = p.get("namespace")?;
            Some(SearchHit {
                id: p.get("id")?.as_i64()?,
                slug: ns.get("slug")?.as_str()?.to_string(),
                owner: ns
                    .get("owner")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: p.get("name")?.as_str()?.to_string(),
                description: p
                    .get("description")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                downloads: p
                    .pointer("/stats/downloads")
                    .and_then(|d| d.as_i64())
                    .unwrap_or(0),
                avatar_url: p
                    .get("avatarUrl")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                category: p
                    .get("category")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                platforms: p
                    .get("supportedPlatforms")
                    .and_then(|o| o.as_object())
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default(),
            })
        })
        .collect();

    Ok(SearchResponse {
        total_hits: pagination
            .get("count")
            .and_then(|c| c.as_u64())
            .unwrap_or(hits.len() as u64),
        limit: pagination
            .get("limit")
            .and_then(|c| c.as_u64())
            .unwrap_or(limit as u64) as u32,
        offset: pagination
            .get("offset")
            .and_then(|c| c.as_u64())
            .unwrap_or(q.offset as u64) as u32,
        hits,
    })
}

pub async fn list_versions(slug: &str, q: &VersionsQuery) -> anyhow::Result<Vec<VersionInfo>> {
    let platform = q.platform.to_ascii_uppercase();
    let limit = q.limit.clamp(1, 50);
    let mut url = reqwest::Url::parse(&format!("{API}/projects/{slug}/versions"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("limit", &limit.to_string());
        qp.append_pair("offset", "0");
        qp.append_pair("platform", &platform);
        qp.append_pair("includeChannelInfo", "false");
    }

    let v: Value = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut out = Vec::new();
    for ver in v
        .get("result")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let downloads = ver.get("downloads").and_then(|d| d.as_object());
        let Some(plat) = downloads.and_then(|d| d.get(&platform).or_else(|| d.values().next()))
        else {
            continue;
        };
        let download_url = plat
            .get("downloadUrl")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                // Fallback official download route
                let name = ver.get("name")?.as_str()?;
                Some(format!(
                    "{API}/projects/{slug}/versions/{name}/{platform}/download"
                ))
            });
        let Some(download_url) = download_url else {
            continue;
        };
        let file_info = plat.get("fileInfo");
        out.push(VersionInfo {
            id: ver.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            name: ver
                .get("name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            platform: platform.clone(),
            downloads: plat
                .get("downloads")
                .and_then(|d| d.as_i64())
                .or_else(|| {
                    ver.get("stats")
                        .and_then(|s| s.get("totalDownloads"))
                        .and_then(|d| d.as_i64())
                })
                .unwrap_or(0),
            filename: file_info
                .and_then(|f| f.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("plugin.jar")
                .to_string(),
            download_url,
            size: file_info
                .and_then(|f| f.get("sizeBytes"))
                .and_then(|s| s.as_u64())
                .unwrap_or(0),
            game_versions: ver
                .get("platformDependencies")
                .and_then(|p| p.get(&platform))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

pub async fn pick_version(req: &InstallRequest) -> anyhow::Result<VersionInfo> {
    let platform = req.platform.to_ascii_uppercase();
    let mut versions = list_versions(
        &req.slug,
        &VersionsQuery {
            platform: platform.clone(),
            limit: 25,
        },
    )
    .await?;
    if versions.is_empty() {
        anyhow::bail!("no Hangar versions for {} / {}", req.slug, platform);
    }
    if let Some(want) = req.version.as_ref().filter(|s| !s.is_empty() && *s != "latest") {
        return versions
            .into_iter()
            .find(|v| v.name == *want || v.id.to_string() == *want)
            .ok_or_else(|| anyhow::anyhow!("Hangar version not found: {want}"));
    }
    if let Some(idx) = versions
        .iter()
        .position(|v| !v.name.to_ascii_uppercase().contains("SNAPSHOT"))
    {
        return Ok(versions.swap_remove(idx));
    }
    Ok(versions.remove(0))
}

pub async fn download_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client().get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    if bytes.len() > 512 * 1024 * 1024 {
        anyhow::bail!("Hangar file too large (>512MiB)");
    }
    if bytes.len() < 64 {
        anyhow::bail!("Hangar download too small / empty");
    }
    Ok(bytes.to_vec())
}
