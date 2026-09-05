//! Server jar version listing + install (Paper Fill v3 + Mojang Vanilla).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const USER_AGENT: &str = "Cocktail-Manager/0.1 (https://github.com/cocktail; contact=dev@local)";

#[derive(Debug, Serialize)]
pub struct CoreVersion {
    pub id: String,
    pub core: String,
    pub latest: bool,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub core: String,
    pub version: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("http client")
}

pub async fn list_versions(core: &str) -> anyhow::Result<Vec<CoreVersion>> {
    match core {
        "paper" => list_paper_versions().await,
        "vanilla" => list_vanilla_versions().await,
        other => anyhow::bail!("unsupported core: {other} (supported: paper, vanilla)"),
    }
}

async fn list_paper_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let url = "https://fill.papermc.io/v3/projects/paper";
    let v: Value = client().get(url).send().await?.error_for_status()?.json().await?;
    let mut out = Vec::new();
    let Some(groups) = v.get("versions").and_then(|x| x.as_object()) else {
        anyhow::bail!("unexpected paper API response");
    };
    for (_group, versions) in groups {
        if let Some(arr) = versions.as_array() {
            for ver in arr {
                if let Some(id) = ver.as_str() {
                    let lower = id.to_ascii_lowercase();
                    if lower.contains("rc")
                        || lower.contains("pre")
                        || lower.contains("snapshot")
                        || lower.contains("alpha")
                        || lower.contains("beta")
                    {
                        continue;
                    }
                    out.push(CoreVersion {
                        id: id.to_string(),
                        core: "paper".into(),
                        latest: false,
                    });
                }
            }
        }
    }
    // Fill groups are newest-first; keep API order and mark head as latest.
    out.truncate(40);
    if let Some(first) = out.first_mut() {
        first.latest = true;
    }
    Ok(out)
}

async fn list_vanilla_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
    let v: Value = client().get(url).send().await?.error_for_status()?.json().await?;
    let latest = v
        .pointer("/latest/release")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let mut out = Vec::new();
    if let Some(arr) = v.get("versions").and_then(|x| x.as_array()) {
        for ver in arr {
            let id = ver.get("id").and_then(|x| x.as_str()).unwrap_or("");
            let ty = ver.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if ty != "release" {
                continue;
            }
            out.push(CoreVersion {
                id: id.to_string(),
                core: "vanilla".into(),
                latest: id == latest,
            });
            if out.len() >= 40 {
                break;
            }
        }
    }
    Ok(out)
}

pub async fn download_and_install(
    workdir: &str,
    core: &str,
    version: &str,
) -> anyhow::Result<(String, Vec<String>)> {
    fs::create_dir_all(workdir)?;
    let jar_path = Path::new(workdir).join("server.jar");
    let url = match core {
        "paper" => resolve_paper_download_url(version).await?,
        "vanilla" => resolve_vanilla_download_url(version).await?,
        other => anyhow::bail!("unsupported core: {other}"),
    };

    tracing::info!(%url, %core, %version, "downloading server jar");
    let bytes = client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if bytes.len() < 1024 {
        anyhow::bail!("downloaded jar looks too small");
    }
    fs::write(&jar_path, &bytes)?;
    tracing::info!(path = %jar_path.display(), size = bytes.len(), "server jar installed");

    Ok((
        "java".into(),
        vec!["-jar".into(), "server.jar".into(), "nogui".into()],
    ))
}

async fn resolve_paper_download_url(version: &str) -> anyhow::Result<String> {
    let url = format!("https://fill.papermc.io/v3/projects/paper/versions/{version}/builds");
    let builds: Value = client().get(&url).send().await?.error_for_status()?.json().await?;
    let arr = builds
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("paper builds response is not an array"))?;

    // Prefer STABLE, then any with server:default url (newest first typically).
    let mut chosen: Option<String> = None;
    for build in arr {
        let channel = build
            .get("channel")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let dl = build
            .pointer("/downloads/server:default/url")
            .and_then(|u| u.as_str());
        if let Some(u) = dl {
            if channel.eq_ignore_ascii_case("STABLE") {
                return Ok(u.to_string());
            }
            if chosen.is_none() {
                chosen = Some(u.to_string());
            }
        }
    }
    chosen.ok_or_else(|| anyhow::anyhow!("no downloadable paper build for {version}"))
}

async fn resolve_vanilla_download_url(version: &str) -> anyhow::Result<String> {
    let manifest: Value = client()
        .get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let meta_url = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some(version))
        .and_then(|v| v.get("url").and_then(|u| u.as_str()))
        .ok_or_else(|| anyhow::anyhow!("vanilla version not found: {version}"))?
        .to_string();

    let detail: Value = client()
        .get(&meta_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    detail
        .pointer("/downloads/server/url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("vanilla server download missing for {version}"))
}
