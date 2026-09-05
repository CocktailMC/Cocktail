//! Modrinth API client — https://docs.modrinth.com/api/
//! Search/read need no token; a uniquely-identifying User-Agent is required.

use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://api.modrinth.com/v2";
const USER_AGENT: &str =
    "Cocktail-Manager/0.1 (contact=dev@local; +https://docs.modrinth.com/api/)";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("http client")
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub query: String,
    /// plugin | mod | datapack | modpack
    #[serde(default = "default_project_type")]
    pub project_type: String,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_project_type() -> String {
    "plugin".into()
}

fn default_limit() -> u32 {
    20
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub project_type: String,
    pub downloads: i64,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub versions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u64,
}

#[derive(Debug, Deserialize)]
pub struct VersionsQuery {
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct VersionInfo {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: String,
    pub downloads: i64,
    pub primary_filename: String,
    pub primary_url: String,
    pub primary_size: u64,
}

#[derive(Debug, Deserialize)]
pub struct InstallModrinthRequest {
    pub project_id: String,
    #[serde(default)]
    pub version_id: Option<String>,
    /// plugins | mods
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub project_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub path: String,
    pub filename: String,
    pub size: u64,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub target: String,
}

pub async fn search(q: &SearchQuery) -> anyhow::Result<SearchResponse> {
    let limit = q.limit.clamp(1, 50);
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", q.project_type)]];
    if let Some(ver) = q.game_version.as_ref().filter(|s| !s.is_empty()) {
        facets.push(vec![format!("versions:{ver}")]);
    }
    if let Some(loader) = q.loader.as_ref().filter(|s| !s.is_empty()) {
        facets.push(vec![format!("categories:{loader}")]);
    }

    let facets_json = serde_json::to_string(&facets)?;
    let mut url = reqwest::Url::parse(&format!("{API}/search"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("query", &q.query);
        qp.append_pair("limit", &limit.to_string());
        qp.append_pair("offset", &q.offset.to_string());
        qp.append_pair("index", "relevance");
        qp.append_pair("facets", &facets_json);
    }

    let v: Value = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let hits = v
        .get("hits")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|h| {
            Some(SearchHit {
                project_id: h.get("project_id")?.as_str()?.to_string(),
                slug: h
                    .get("slug")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: h.get("title")?.as_str()?.to_string(),
                description: h
                    .get("description")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                author: h
                    .get("author")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                project_type: h
                    .get("project_type")
                    .and_then(|s| s.as_str())
                    .unwrap_or("mod")
                    .to_string(),
                downloads: h.get("downloads").and_then(|d| d.as_i64()).unwrap_or(0),
                icon_url: h
                    .get("icon_url")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                categories: h
                    .get("categories")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                versions: h
                    .get("versions")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .take(8)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();

    Ok(SearchResponse {
        total_hits: v.get("total_hits").and_then(|t| t.as_u64()).unwrap_or(0),
        offset: v
            .get("offset")
            .and_then(|t| t.as_u64())
            .unwrap_or(q.offset as u64) as u32,
        limit: v
            .get("limit")
            .and_then(|t| t.as_u64())
            .unwrap_or(limit as u64) as u32,
        hits,
    })
}

pub async fn list_versions(
    id_or_slug: &str,
    q: &VersionsQuery,
) -> anyhow::Result<Vec<VersionInfo>> {
    let mut url = reqwest::Url::parse(&format!("{API}/project/{id_or_slug}/version"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("include_changelog", "false");
        if let Some(ver) = q.game_version.as_ref().filter(|s| !s.is_empty()) {
            qp.append_pair("game_versions", &serde_json::to_string(&vec![ver])?);
        }
        if let Some(loader) = q.loader.as_ref().filter(|s| !s.is_empty()) {
            qp.append_pair("loaders", &serde_json::to_string(&vec![loader])?);
        }
    }

    let arr: Vec<Value> = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut out = Vec::new();
    for v in arr {
        if let Ok(info) = parse_version_value(&v) {
            out.push(info);
        }
    }
    Ok(out)
}

fn parse_version_value(v: &Value) -> anyhow::Result<VersionInfo> {
    let files = v
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow::anyhow!("version has no files"))?;
    let file = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true))
        .or_else(|| files.first())
        .ok_or_else(|| anyhow::anyhow!("version has no files"))?;
    Ok(VersionInfo {
        id: v
            .get("id")
            .and_then(|s| s.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing version id"))?
            .into(),
        name: v.get("name").and_then(|s| s.as_str()).unwrap_or("").into(),
        version_number: v
            .get("version_number")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .into(),
        version_type: v
            .get("version_type")
            .and_then(|s| s.as_str())
            .unwrap_or("release")
            .into(),
        game_versions: v
            .get("game_versions")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        loaders: v
            .get("loaders")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        date_published: v
            .get("date_published")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .into(),
        downloads: v.get("downloads").and_then(|d| d.as_i64()).unwrap_or(0),
        primary_filename: file
            .get("filename")
            .and_then(|s| s.as_str())
            .unwrap_or("file.jar")
            .into(),
        primary_url: file
            .get("url")
            .and_then(|s| s.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing download url"))?
            .into(),
        primary_size: file.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
    })
}

pub async fn pick_version(req: &InstallModrinthRequest) -> anyhow::Result<VersionInfo> {
    if let Some(vid) = req.version_id.as_ref().filter(|s| !s.is_empty()) {
        let url = format!("{API}/version/{vid}");
        let v: Value = client()
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        return parse_version_value(&v);
    }
    let mut versions = list_versions(
        &req.project_id,
        &VersionsQuery {
            game_version: req.game_version.clone(),
            loader: req.loader.clone(),
        },
    )
    .await?;
    if versions.is_empty() {
        anyhow::bail!("no matching Modrinth version for filters");
    }
    if let Some(idx) = versions.iter().position(|v| v.version_type == "release") {
        return Ok(versions.swap_remove(idx));
    }
    Ok(versions.remove(0))
}

pub async fn download_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client().get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    if bytes.len() > 512 * 1024 * 1024 {
        anyhow::bail!("modrinth file too large (>512MiB)");
    }
    Ok(bytes.to_vec())
}

pub fn infer_target(project_type: &str, loaders: &[String], explicit: Option<&str>) -> String {
    if let Some(t) = explicit.filter(|s| *s == "plugins" || *s == "mods") {
        return t.to_string();
    }
    let loaders_l: Vec<_> = loaders.iter().map(|s| s.to_ascii_lowercase()).collect();
    if loaders_l.iter().any(|l| {
        matches!(
            l.as_str(),
            "paper"
                | "spigot"
                | "bukkit"
                | "purpur"
                | "folia"
                | "waterfall"
                | "velocity"
                | "bungeecord"
        )
    }) || project_type == "plugin"
    {
        return "plugins".into();
    }
    "mods".into()
}
