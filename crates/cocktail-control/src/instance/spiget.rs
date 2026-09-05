//! Spiget API client — https://spiget.org / SpigotMC resources mirror.
//! OpenAPI: https://raw.githubusercontent.com/SpiGetOrg/Documentation/master/swagger.yml

use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://api.spiget.org/v2";
const USER_AGENT: &str =
    "Cocktail-Manager/0.1 (contact=dev@local; +https://spiget.org)";

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
    #[serde(default = "default_size")]
    pub size: u32,
    #[serde(default)]
    pub page: u32,
}

fn default_size() -> u32 {
    20
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub name: String,
    pub tag: String,
    pub downloads: i64,
    pub external: bool,
    pub premium: bool,
    pub file_type: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub tested_versions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, Clone)]
pub struct VersionInfo {
    pub id: i64,
    pub name: String,
    pub downloads: i64,
    pub release_date: i64,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub resource_id: i64,
    #[serde(default)]
    pub version_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub path: String,
    pub filename: String,
    pub size: u64,
    pub resource_id: i64,
    pub version_id: Option<i64>,
    pub version_name: String,
}

pub async fn search(q: &SearchQuery) -> anyhow::Result<SearchResponse> {
    let size = q.size.clamp(1, 50);
    let page = q.page.max(1);
    let query = if q.query.trim().is_empty() {
        "plugin".to_string()
    } else {
        q.query.trim().to_string()
    };
    let mut url =
        reqwest::Url::parse(&format!("{API}/search/resources/{}", urlencoding_path(&query)))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("size", &size.to_string());
        qp.append_pair("page", &page.to_string());
        qp.append_pair("field", "name");
    }

    let arr: Vec<Value> = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let hits = arr
        .into_iter()
        .filter_map(|r| {
            let icon_url = resolve_icon(r.get("icon"), r.get("id")?.as_i64()?);
            Some(SearchHit {
                id: r.get("id")?.as_i64()?,
                name: r.get("name")?.as_str()?.to_string(),
                tag: r.get("tag").and_then(|s| s.as_str()).unwrap_or("").into(),
                downloads: r.get("downloads").and_then(|d| d.as_i64()).unwrap_or(0),
                external: r.get("external").and_then(|b| b.as_bool()).unwrap_or(false),
                premium: r.get("premium").and_then(|b| b.as_bool()).unwrap_or(false),
                file_type: r
                    .pointer("/file/type")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .into(),
                author: r
                    .pointer("/author/id")
                    .and_then(|i| i.as_i64())
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                icon_url,
                tested_versions: r
                    .get("testedVersions")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();

    Ok(SearchResponse { hits })
}

fn resolve_icon(icon: Option<&Value>, resource_id: i64) -> Option<String> {
    // Prefer embedded base64 (works offline / no hotlink issues).
    if let Some(data) = icon
        .and_then(|i| i.get("data"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
    {
        let mime = if data.starts_with("/9j/") {
            "image/jpeg"
        } else if data.starts_with("R0lGOD") {
            "image/gif"
        } else {
            "image/png"
        };
        return Some(format!("data:{mime};base64,{data}"));
    }
    // Relative SpigotMC path — browsers often block hotlinking; use our proxy instead.
    // Absolute http(s) still proxied for consistency.
    let _rel = icon
        .and_then(|i| i.get("url"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty());
    Some(format!("/api/v1/spiget/resources/{resource_id}/icon"))
}

/// Fetch resource icon bytes (proxies Spiget / SpigotMC).
pub async fn fetch_icon(resource_id: i64) -> anyhow::Result<(String, Vec<u8>)> {
    // 1) Try Spiget API icon endpoint
    let api_url = format!("{API}/resources/{resource_id}/icon");
    if let Ok(resp) = client().get(&api_url).send().await {
        if resp.status().is_success() {
            let ctype = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("image/png")
                .to_string();
            let bytes = resp.bytes().await?.to_vec();
            if bytes.len() > 32 && !bytes.starts_with(b"<") {
                return Ok((ctype, bytes));
            }
        }
    }

    // 2) Fall back to resource meta relative URL on spigotmc.org
    let meta = resource_meta(resource_id).await?;
    if let Some(path) = meta
        .pointer("/icon/url")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
    {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("https://www.spigotmc.org/{}", path.trim_start_matches('/'))
        };
        let resp = client().get(&url).send().await?.error_for_status()?;
        let ctype = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();
        let bytes = resp.bytes().await?.to_vec();
        if bytes.len() > 32 {
            return Ok((ctype, bytes));
        }
    }

    // 3) Embedded base64 in meta
    if let Some(data) = meta
        .pointer("/icon/data")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
    {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| anyhow::anyhow!("icon base64: {e}"))?;
        let ctype = if bytes.starts_with(&[0xFF, 0xD8]) {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF") {
            "image/gif"
        } else {
            "image/png"
        };
        return Ok((ctype.into(), bytes));
    }

    anyhow::bail!("no icon for resource {resource_id}")
}

fn urlencoding_path(s: &str) -> String {
    // Spiget expects path-encoded query segment
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".into(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

pub async fn list_versions(resource_id: i64) -> anyhow::Result<Vec<VersionInfo>> {
    let mut url = reqwest::Url::parse(&format!("{API}/resources/{resource_id}/versions"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("size", "25");
        qp.append_pair("sort", "-releaseDate");
    }
    let arr: Vec<Value> = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(arr
        .into_iter()
        .filter_map(|v| {
            Some(VersionInfo {
                id: v.get("id")?.as_i64()?,
                name: v.get("name")?.as_str()?.to_string(),
                downloads: v.get("downloads").and_then(|d| d.as_i64()).unwrap_or(0),
                release_date: v.get("releaseDate").and_then(|d| d.as_i64()).unwrap_or(0),
            })
        })
        .collect())
}

pub async fn resource_meta(resource_id: i64) -> anyhow::Result<Value> {
    let url = format!("{API}/resources/{resource_id}");
    Ok(client().get(url).send().await?.error_for_status()?.json().await?)
}

/// Resolve download URL / bytes for a Spiget resource.
pub async fn download_resource(
    req: &InstallRequest,
) -> anyhow::Result<(Vec<u8>, String, Option<i64>, String)> {
    let meta = resource_meta(req.resource_id).await?;
    if meta.get("premium").and_then(|b| b.as_bool()) == Some(true) {
        anyhow::bail!("Spiget premium resources cannot be downloaded via API");
    }

    let version_id = req.version_id;
    let version_name = if let Some(vid) = version_id {
        list_versions(req.resource_id)
            .await?
            .into_iter()
            .find(|v| v.id == vid)
            .map(|v| v.name)
            .unwrap_or_else(|| vid.to_string())
    } else {
        meta.pointer("/version/name")
            .and_then(|s| s.as_str())
            .unwrap_or("latest")
            .to_string()
    };

    let external = meta.get("external").and_then(|b| b.as_bool()).unwrap_or(false);
    let file_type = meta
        .pointer("/file/type")
        .and_then(|s| s.as_str())
        .unwrap_or(".jar");

    // Prefer direct external jar URL when present
    if external {
        if let Some(ext) = meta
            .pointer("/file/externalUrl")
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
        {
            let bytes = download_url(ext).await?;
            let name = filename_from_url(ext).unwrap_or_else(|| {
                format!("spiget-{}.jar", req.resource_id)
            });
            return Ok((bytes, name, version_id, version_name));
        }
        anyhow::bail!(
            "resource is externally hosted without a direct jar URL; download from SpigotMC manually"
        );
    }

    if file_type != ".jar" && !file_type.is_empty() && file_type != "jar" {
        anyhow::bail!("unsupported Spiget file type: {file_type}");
    }

    let url = if let Some(vid) = version_id {
        format!("{API}/resources/{}/versions/{}/download", req.resource_id, vid)
    } else {
        format!("{API}/resources/{}/download", req.resource_id)
    };

    let bytes = download_url(&url).await?;
    let name = format!(
        "spiget-{}-{}.jar",
        req.resource_id,
        sanitize(&version_name)
    );
    Ok((bytes, name, version_id, version_name))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

fn filename_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next()?;
    let name = path.rsplit('/').next()?;
    if name.to_ascii_lowercase().ends_with(".jar") {
        Some(name.to_string())
    } else {
        None
    }
}

async fn download_url(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client().get(url).send().await?.error_for_status()?;
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = resp.bytes().await?;
    if bytes.len() > 512 * 1024 * 1024 {
        anyhow::bail!("Spiget file too large (>512MiB)");
    }
    if bytes.len() < 64 {
        anyhow::bail!("Spiget download empty (may be HTML / rate-limited)");
    }
    // Heuristic: jar files start with PK (zip)
    if !bytes.starts_with(b"PK") {
        if ctype.contains("text/html") || bytes.starts_with(b"<!") || bytes.starts_with(b"<html") {
            anyhow::bail!(
                "Spiget returned HTML instead of jar (external/premium/login wall or rate-limit)"
            );
        }
    }
    Ok(bytes.to_vec())
}
