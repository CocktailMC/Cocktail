//! Server core listing + install (Paper/Folia, Vanilla, Fabric, Forge, hybrids, …).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const USER_AGENT: &str = "Cocktail-Manager/0.1 (https://github.com/cocktail; contact=dev@local)";
const MAX_LIST: usize = 40;

const SUPPORTED: &[&str] = &[
    "paper", "folia", "purpur", "leaves", "vanilla", "fabric", "quilt", "forge", "neoforge",
    "mohist", "banner", "arclight",
];

#[derive(Debug, Serialize)]
pub struct CoreVersion {
    pub id: String,
    pub core: String,
    pub latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CoreLoader {
    pub id: String,
    pub latest: bool,
    pub recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub core: String,
    pub version: String,
    #[serde(default)]
    pub loader: Option<String>,
}

pub fn is_known_core(core: &str) -> bool {
    SUPPORTED.contains(&core)
}

pub fn core_needs_eula(core: &str) -> bool {
    core != "demo"
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(180))
        .build()
        .expect("http client")
}

pub async fn list_versions(core: &str) -> anyhow::Result<Vec<CoreVersion>> {
    match core {
        "paper" => list_fill_versions("paper").await,
        "folia" => list_fill_versions("folia").await,
        "purpur" => list_purpur_versions().await,
        "leaves" => list_leaves_versions().await,
        "vanilla" => list_vanilla_versions().await,
        "fabric" => list_fabric_game_versions().await,
        "quilt" => list_quilt_game_versions().await,
        "forge" => list_forge_versions().await,
        "neoforge" => list_neoforge_versions().await,
        "mohist" => list_mohist_project("mohist").await,
        "banner" => list_mohist_project("banner").await,
        "arclight" => list_arclight_versions().await,
        other => anyhow::bail!(
            "unsupported core: {other} (supported: {})",
            SUPPORTED.join(", ")
        ),
    }
}

pub async fn list_loaders(core: &str, version: &str) -> anyhow::Result<Vec<CoreLoader>> {
    match core {
        "fabric" => list_fabric_loaders(version).await,
        "quilt" => list_quilt_loaders(version).await,
        "forge" => list_forge_loaders(version).await,
        "neoforge" => list_neoforge_loaders(version).await,
        "arclight" => list_arclight_loaders(version).await,
        _ => Ok(Vec::new()),
    }
}

fn opt_loader(loader: Option<&str>) -> Option<&str> {
    loader.map(str::trim).filter(|s| !s.is_empty())
}

pub async fn download_and_install(
    workdir: &str,
    core: &str,
    version: &str,
    loader: Option<&str>,
) -> anyhow::Result<(String, Vec<String>)> {
    fs::create_dir_all(workdir)?;
    let loader = opt_loader(loader);
    match core {
        "forge" => install_forge(workdir, version, loader).await,
        "neoforge" => install_neoforge(workdir, version, loader).await,
        "quilt" => install_quilt(workdir, version, loader).await,
        "fabric" => {
            let url = resolve_fabric_server_jar(version, loader).await?;
            write_server_jar(workdir, &url).await
        }
        "paper" | "folia" | "purpur" | "leaves" | "vanilla" | "mohist" | "banner" | "arclight" => {
            let url = match core {
                "paper" => resolve_fill_download_url("paper", version).await?,
                "folia" => resolve_fill_download_url("folia", version).await?,
                "purpur" => resolve_purpur_download_url(version).await?,
                "leaves" => resolve_leaves_download_url(version).await?,
                "vanilla" => resolve_vanilla_download_url(version).await?,
                "mohist" => resolve_mohist_download_url("mohist", version).await?,
                "banner" => resolve_mohist_download_url("banner", version).await?,
                "arclight" => resolve_arclight_download_url(version, loader).await?,
                _ => unreachable!(),
            };
            write_server_jar(workdir, &url).await
        }
        other => anyhow::bail!("unsupported core: {other}"),
    }
}

fn mark_latest(mut out: Vec<CoreVersion>) -> Vec<CoreVersion> {
    if let Some(first) = out.first_mut() {
        first.latest = true;
    }
    out
}

fn mark_latest_loaders(mut out: Vec<CoreLoader>) -> Vec<CoreLoader> {
    if let Some(first) = out.first_mut() {
        first.latest = true;
    }
    out
}

fn extract_maven_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + 9..];
        if let Some(end) = rest.find("</version>") {
            out.push(rest[..end].trim().to_string());
            rest = &rest[end + 10..];
        } else {
            break;
        }
    }
    out
}

fn is_unstable(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    l.contains("rc")
        || l.contains("pre")
        || l.contains("snapshot")
        || l.contains("alpha")
        || l.contains("beta")
        || l.contains("hack")
        || l.contains("experimental")
}

fn parse_mc_parts(id: &str) -> Vec<u32> {
    id.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn cmp_mc_version(a: &str, b: &str) -> std::cmp::Ordering {
    parse_mc_parts(b).cmp(&parse_mc_parts(a))
}

fn take_newest(mut ids: Vec<String>, core: &str) -> Vec<CoreVersion> {
    ids.sort_by(|a, b| cmp_mc_version(a, b));
    ids.dedup();
    ids.truncate(MAX_LIST);
    mark_latest(
        ids.into_iter()
            .map(|id| CoreVersion {
                id,
                core: core.into(),
                latest: false,
                label: None,
            })
            .collect(),
    )
}

async fn get_json(url: &str) -> anyhow::Result<Value> {
    Ok(client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn write_server_jar(workdir: &str, url: &str) -> anyhow::Result<(String, Vec<String>)> {
    let jar_path = Path::new(workdir).join("server.jar");
    tracing::info!(%url, "downloading server jar");
    let bytes = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if bytes.len() < 1024 {
        anyhow::bail!("downloaded jar looks too small ({} bytes)", bytes.len());
    }
    fs::write(&jar_path, &bytes)?;
    tracing::info!(path = %jar_path.display(), size = bytes.len(), "server jar installed");
    Ok(crate::util::java_jar_startup("server.jar"))
}

async fn download_file(url: &str, dest: &Path) -> anyhow::Result<u64> {
    tracing::info!(%url, path = %dest.display(), "downloading");
    let bytes = client()
        .get(url)
        .timeout(Duration::from_secs(300))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if bytes.len() < 1024 {
        anyhow::bail!("downloaded file looks too small ({} bytes)", bytes.len());
    }
    fs::write(dest, &bytes)?;
    Ok(bytes.len() as u64)
}

// --- Paper Fill (paper / folia) ---

async fn list_fill_versions(project: &str) -> anyhow::Result<Vec<CoreVersion>> {
    let url = format!("https://fill.papermc.io/v3/projects/{project}");
    let v = get_json(&url).await?;
    let mut out = Vec::new();
    let Some(groups) = v.get("versions").and_then(|x| x.as_object()) else {
        anyhow::bail!("unexpected {project} API response");
    };
    for (_group, versions) in groups {
        if let Some(arr) = versions.as_array() {
            for ver in arr {
                if let Some(id) = ver.as_str() {
                    if is_unstable(id) {
                        continue;
                    }
                    out.push(CoreVersion {
                        id: id.to_string(),
                        core: project.into(),
                        latest: false,
                        label: None,
                    });
                }
            }
        }
    }
    out.truncate(MAX_LIST);
    Ok(mark_latest(out))
}

async fn resolve_fill_download_url(project: &str, version: &str) -> anyhow::Result<String> {
    let url = format!("https://fill.papermc.io/v3/projects/{project}/versions/{version}/builds");
    let builds = get_json(&url).await?;
    let arr = builds
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("{project} builds response is not an array"))?;

    let mut chosen: Option<String> = None;
    for build in arr {
        let channel = build.get("channel").and_then(|c| c.as_str()).unwrap_or("");
        let dl = build
            .pointer("/downloads/server:default/url")
            .and_then(|u| u.as_str());
        if let Some(u) = dl {
            if channel.eq_ignore_ascii_case("STABLE") || channel.eq_ignore_ascii_case("RECOMMENDED")
            {
                return Ok(u.to_string());
            }
            if chosen.is_none() {
                chosen = Some(u.to_string());
            }
        }
    }
    chosen.ok_or_else(|| anyhow::anyhow!("no downloadable {project} build for {version}"))
}

// --- Vanilla ---

async fn list_vanilla_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json").await?;
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
                label: None,
            });
            if out.len() >= MAX_LIST {
                break;
            }
        }
    }
    Ok(out)
}

async fn resolve_vanilla_download_url(version: &str) -> anyhow::Result<String> {
    let manifest =
        get_json("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json").await?;
    let meta_url = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some(version))
        .and_then(|v| v.get("url").and_then(|u| u.as_str()))
        .ok_or_else(|| anyhow::anyhow!("vanilla version not found: {version}"))?
        .to_string();

    let detail = get_json(&meta_url).await?;
    detail
        .pointer("/downloads/server/url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("vanilla server download missing for {version}"))
}

// --- Purpur / Leaves ---

async fn list_purpur_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://api.purpurmc.org/v2/purpur").await?;
    let ids = v
        .get("versions")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .filter(|id| !is_unstable(id))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, "purpur"))
}

async fn resolve_purpur_download_url(version: &str) -> anyhow::Result<String> {
    Ok(format!(
        "https://api.purpurmc.org/v2/purpur/{version}/latest/download"
    ))
}

async fn list_leaves_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://api.leavesmc.org/v2/projects/leaves").await?;
    let ids = v
        .get("versions")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .filter(|id| !is_unstable(id))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, "leaves"))
}

async fn resolve_leaves_download_url(version: &str) -> anyhow::Result<String> {
    let url = format!("https://api.leavesmc.org/v2/projects/leaves/versions/{version}/builds");
    let v = get_json(&url).await?;
    let builds = v
        .get("builds")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("leaves builds missing for {version}"))?;
    let pick = builds
        .iter()
        .rev()
        .find(|b| {
            b.get("channel")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .eq_ignore_ascii_case("default")
        })
        .or(builds.last())
        .ok_or_else(|| anyhow::anyhow!("no leaves build for {version}"))?;
    let build = pick
        .get("build")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| anyhow::anyhow!("leaves build id missing"))?;
    Ok(format!(
        "https://api.leavesmc.org/v2/projects/leaves/versions/{version}/builds/{build}/downloads/application"
    ))
}

// --- Fabric ---

async fn list_fabric_game_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://meta.fabricmc.net/v2/versions/game").await?;
    let ids = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row.get("stable").and_then(|s| s.as_bool()).unwrap_or(false))
        .filter_map(|row| row.get("version").and_then(|x| x.as_str()))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, "fabric"))
}

async fn list_fabric_loaders(mc: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let v = get_json(&format!("https://meta.fabricmc.net/v2/versions/loader/{mc}")).await?;
    let mut out = Vec::new();
    for row in v.as_array().into_iter().flatten() {
        let Some(ver) = row.pointer("/loader/version").and_then(|x| x.as_str()) else {
            continue;
        };
        let stable = row
            .pointer("/loader/stable")
            .and_then(|x| x.as_bool())
            .unwrap_or(!is_unstable(ver));
        if !stable || is_unstable(ver) {
            continue;
        }
        out.push(CoreLoader {
            id: ver.to_string(),
            latest: false,
            recommended: false,
            label: None,
        });
        if out.len() >= MAX_LIST {
            break;
        }
    }
    Ok(mark_latest_loaders(out))
}

async fn latest_fabric_loader(mc: &str) -> anyhow::Result<String> {
    let v = get_json(&format!("https://meta.fabricmc.net/v2/versions/loader/{mc}")).await?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("fabric loader list missing for {mc}"))?;
    let pick = arr
        .iter()
        .find(|row| {
            let ver = row
                .pointer("/loader/version")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let stable = row
                .pointer("/loader/stable")
                .and_then(|x| x.as_bool())
                .unwrap_or(!is_unstable(ver));
            stable && !is_unstable(ver)
        })
        .or(arr.first())
        .ok_or_else(|| anyhow::anyhow!("no fabric loader for {mc}"))?;
    pick.pointer("/loader/version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("fabric loader version missing"))
}

async fn latest_fabric_installer() -> anyhow::Result<String> {
    let v = get_json("https://meta.fabricmc.net/v2/versions/installer").await?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("fabric installer list missing"))?;
    let pick = arr
        .iter()
        .find(|row| row.get("stable").and_then(|s| s.as_bool()).unwrap_or(false))
        .or(arr.first())
        .ok_or_else(|| anyhow::anyhow!("no fabric installer"))?;
    pick.get("version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("fabric installer version missing"))
}

async fn resolve_fabric_server_jar(mc: &str, loader: Option<&str>) -> anyhow::Result<String> {
    let loader = match loader {
        Some(v) => v.to_string(),
        None => latest_fabric_loader(mc).await?,
    };
    let installer = latest_fabric_installer().await?;
    Ok(format!(
        "https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/{installer}/server/jar"
    ))
}

// --- Quilt (installer) ---

async fn list_quilt_game_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://meta.quiltmc.org/v3/versions/game").await?;
    let ids = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row.get("stable").and_then(|s| s.as_bool()).unwrap_or(false))
        .filter_map(|row| row.get("version").and_then(|x| x.as_str()))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, "quilt"))
}

async fn list_quilt_loaders(mc: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let v = get_json(&format!("https://meta.quiltmc.org/v3/versions/loader/{mc}")).await?;
    let mut out = Vec::new();
    for row in v.as_array().into_iter().flatten() {
        let Some(ver) = row.pointer("/loader/version").and_then(|x| x.as_str()) else {
            continue;
        };
        if is_unstable(ver) {
            continue;
        }
        out.push(CoreLoader {
            id: ver.to_string(),
            latest: false,
            recommended: false,
            label: None,
        });
        if out.len() >= MAX_LIST {
            break;
        }
    }
    Ok(mark_latest_loaders(out))
}

async fn latest_quilt_loader(mc: &str) -> anyhow::Result<String> {
    let v = get_json(&format!("https://meta.quiltmc.org/v3/versions/loader/{mc}")).await?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("quilt loader list missing for {mc}"))?;
    let pick = arr
        .iter()
        .find(|row| {
            let ver = row
                .pointer("/loader/version")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            !is_unstable(ver)
        })
        .or(arr.first())
        .ok_or_else(|| anyhow::anyhow!("no quilt loader for {mc}"))?;
    pick.pointer("/loader/version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("quilt loader version missing"))
}

async fn latest_quilt_installer_url() -> anyhow::Result<(String, String)> {
    let v = get_json("https://meta.quiltmc.org/v3/versions/installer").await?;
    let pick = v
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("quilt installer list empty"))?;
    let ver = pick
        .get("version")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("quilt installer version missing"))?;
    let url = pick
        .get("url")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("quilt installer url missing"))?;
    Ok((ver.to_string(), url.to_string()))
}

async fn install_quilt(
    workdir: &str,
    mc: &str,
    loader: Option<&str>,
) -> anyhow::Result<(String, Vec<String>)> {
    let loader = match loader {
        Some(v) => v.to_string(),
        None => latest_quilt_loader(mc).await?,
    };
    let (_ver, url) = latest_quilt_installer_url().await?;
    let installer = Path::new(workdir).join("quilt-installer.jar");
    download_file(&url, &installer).await?;
    let java = installer_java(mc).await?;
    run_java_installer(
        &java,
        workdir,
        &installer,
        &[
            "install",
            "server",
            mc,
            "--download-server",
            "--loader-version",
            &loader,
            "--install-dir",
            ".",
        ],
    )
    .await?;
    let _ = fs::remove_file(&installer);
    let launch = Path::new(workdir).join("quilt-server-launch.jar");
    if !launch.exists() {
        anyhow::bail!("quilt installer finished but quilt-server-launch.jar is missing");
    }
    Ok(crate::util::java_jar_startup("quilt-server-launch.jar"))
}

// --- Forge ---

async fn list_forge_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .await?;
    let mut by_mc: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let Some(promos) = v.get("promos").and_then(|x| x.as_object()) else {
        anyhow::bail!("forge promotions missing");
    };
    for (key, val) in promos {
        let Some(forge) = val.as_str() else { continue };
        let recommended = key.ends_with("-recommended");
        let latest = key.ends_with("-latest");
        if !recommended && !latest {
            continue;
        }
        let mc = key
            .trim_end_matches("-recommended")
            .trim_end_matches("-latest")
            .to_string();
        if is_unstable(&mc) {
            continue;
        }
        match by_mc.get(&mc) {
            None => {
                by_mc.insert(mc, (forge.to_string(), recommended));
            }
            Some((_, was_rec)) if recommended && !*was_rec => {
                by_mc.insert(mc, (forge.to_string(), true));
            }
            Some(_) => {}
        }
    }
    let mut rows: Vec<(String, String)> = by_mc.into_iter().map(|(mc, (fg, _))| (mc, fg)).collect();
    rows.sort_by(|a, b| cmp_mc_version(&a.0, &b.0));
    rows.truncate(MAX_LIST);
    Ok(mark_latest(
        rows.into_iter()
            .map(|(mc, forge)| CoreVersion {
                label: Some(format!("{mc} · Forge {forge}")),
                id: mc,
                core: "forge".into(),
                latest: false,
            })
            .collect(),
    ))
}

async fn forge_build_for_mc(mc: &str) -> anyhow::Result<String> {
    let v = get_json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .await?;
    let promos = v
        .get("promos")
        .and_then(|x| x.as_object())
        .ok_or_else(|| anyhow::anyhow!("forge promotions missing"))?;
    let rec = promos
        .get(&format!("{mc}-recommended"))
        .and_then(|x| x.as_str());
    let latest = promos.get(&format!("{mc}-latest")).and_then(|x| x.as_str());
    rec.or(latest)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("no forge build for {mc}"))
}

fn strip_mc_prefix(mc: &str, loader: &str) -> String {
    let prefix = format!("{mc}-");
    loader
        .strip_prefix(&prefix)
        .unwrap_or(loader)
        .to_string()
}

async fn get_text(url: &str) -> anyhow::Result<String> {
    Ok(client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

async fn list_forge_loaders(mc: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let xml =
        get_text("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
            .await?;
    let recommended = forge_build_for_mc(mc).await.ok();
    let prefix = format!("{mc}-");
    let mut ids: Vec<String> = extract_maven_versions(&xml)
        .into_iter()
        .filter(|v| v.starts_with(&prefix) && !is_unstable(v))
        .map(|v| v[prefix.len()..].to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.reverse();
    ids.dedup();
    ids.truncate(MAX_LIST);
    Ok(mark_latest_loaders(
        ids.into_iter()
            .map(|id| {
                let rec = recommended.as_deref() == Some(id.as_str());
                CoreLoader {
                    recommended: rec,
                    label: rec.then(|| "recommended".into()),
                    id,
                    latest: false,
                }
            })
            .collect(),
    ))
}

async fn install_forge(
    workdir: &str,
    mc: &str,
    loader: Option<&str>,
) -> anyhow::Result<(String, Vec<String>)> {
    let build = match loader {
        Some(v) => strip_mc_prefix(mc, v),
        None => forge_build_for_mc(mc).await?,
    };
    let combo = format!("{mc}-{build}");
    let url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{combo}/forge-{combo}-installer.jar"
    );
    let installer = Path::new(workdir).join("forge-installer.jar");
    download_file(&url, &installer).await?;
    let java = installer_java(mc).await?;
    run_java_installer(&java, workdir, &installer, &["--installServer"]).await?;
    let _ = fs::remove_file(&installer);
    detect_modloader_startup(Path::new(workdir))
        .ok_or_else(|| anyhow::anyhow!("forge installer finished but no server launch files were found"))
}

// --- NeoForge ---

fn neoforge_to_mc(ver: &str) -> Option<String> {
    let clean = ver.split('-').next().unwrap_or(ver);
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let major: u32 = parts[0].parse().ok()?;
    if major >= 26 {
        if parts.len() >= 3 && parts[2] != "0" {
            Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
        } else {
            Some(format!("{}.{}", parts[0], parts[1]))
        }
    } else if parts[1] == "0" {
        Some(format!("1.{}", parts[0]))
    } else {
        Some(format!("1.{}.{}", parts[0], parts[1]))
    }
}

async fn list_neoforge_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .await?;
    let arr = v
        .get("versions")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("neoforge maven versions missing"))?;
    let mut best: BTreeMap<String, String> = BTreeMap::new();
    for item in arr.iter().rev() {
        let Some(ver) = item.as_str() else { continue };
        if is_unstable(ver) {
            continue;
        }
        let Some(mc) = neoforge_to_mc(ver) else { continue };
        best.entry(mc).or_insert_with(|| ver.to_string());
    }
    let mut rows: Vec<(String, String)> = best.into_iter().collect();
    rows.sort_by(|a, b| cmp_mc_version(&a.0, &b.0));
    rows.truncate(MAX_LIST);
    Ok(mark_latest(
        rows.into_iter()
            .map(|(mc, nf)| CoreVersion {
                label: Some(format!("{mc} · NeoForge {nf}")),
                id: mc,
                core: "neoforge".into(),
                latest: false,
            })
            .collect(),
    ))
}

async fn list_neoforge_loaders(mc: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let v = get_json("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .await?;
    let arr = v
        .get("versions")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("neoforge maven versions missing"))?;
    let mut ids: Vec<String> = arr
        .iter()
        .filter_map(|x| x.as_str())
        .filter(|ver| !is_unstable(ver) && neoforge_to_mc(ver).as_deref() == Some(mc))
        .map(|s| s.to_string())
        .collect();
    ids.reverse();
    ids.dedup();
    ids.truncate(MAX_LIST);
    Ok(mark_latest_loaders(
        ids.into_iter()
            .map(|id| CoreLoader {
                id,
                latest: false,
                recommended: false,
                label: None,
            })
            .collect(),
    ))
}

async fn neoforge_build_for_mc(mc: &str) -> anyhow::Result<String> {
    let v = get_json("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .await?;
    let arr = v
        .get("versions")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("neoforge maven versions missing"))?;
    arr.iter()
        .rev()
        .filter_map(|x| x.as_str())
        .find(|ver| !is_unstable(ver) && neoforge_to_mc(ver).as_deref() == Some(mc))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("no NeoForge build for {mc}"))
}

async fn install_neoforge(
    workdir: &str,
    mc: &str,
    loader: Option<&str>,
) -> anyhow::Result<(String, Vec<String>)> {
    let build = match loader {
        Some(v) => v.to_string(),
        None => neoforge_build_for_mc(mc).await?,
    };
    let url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{build}/neoforge-{build}-installer.jar"
    );
    let installer = Path::new(workdir).join("neoforge-installer.jar");
    download_file(&url, &installer).await?;
    let java = installer_java(mc).await?;
    run_java_installer(&java, workdir, &installer, &["--installServer"]).await?;
    let _ = fs::remove_file(&installer);
    detect_modloader_startup(Path::new(workdir))
        .ok_or_else(|| anyhow::anyhow!("NeoForge installer finished but no server launch files were found"))
}

fn detect_modloader_startup(workdir: &Path) -> Option<(String, Vec<String>)> {
    let arg_name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    let mut args_files = Vec::new();
    find_files(workdir, arg_name, 8, &mut args_files);
    if args_files.is_empty() && cfg!(windows) {
        find_files(workdir, "unix_args.txt", 8, &mut args_files);
    }
    args_files.sort_by_key(|p| {
        let s = p.to_string_lossy().to_ascii_lowercase();
        let score = if s.contains("neoforged") || s.contains("minecraftforge") {
            0
        } else {
            1
        };
        (score, s.len())
    });
    if let Some(args_file) = args_files.first() {
        let rel = pathdiff(workdir, args_file);
        let jvm = workdir.join("user_jvm_args.txt");
        if !jvm.exists() {
            let _ = fs::write(&jvm, "# Cocktail-managed JVM args\n");
        }
        return Some((
            "java".into(),
            vec![
                "@user_jvm_args.txt".into(),
                format!("@{rel}"),
                "nogui".into(),
            ],
        ));
    }
    let mut jars = Vec::new();
    if let Ok(entries) = fs::read_dir(workdir) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".jar")
                && !name.contains("installer")
                && (name.contains("forge") || name.contains("neoforge") || name.contains("shim"))
            {
                jars.push(p.file_name()?.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    jars.sort();
    let jar = jars.into_iter().next()?;
    Some(crate::util::java_jar_startup(&jar))
}

fn pathdiff(base: &Path, file: &Path) -> String {
    file.strip_prefix(base)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn find_files(dir: &Path, name: &str, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_files(&p, name, depth - 1, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            out.push(p);
        }
    }
}

async fn installer_java(mc: &str) -> anyhow::Result<std::path::PathBuf> {
    crate::java::ensure(
        crate::java::recommended_java_major(Some(mc)),
        crate::java::ImageType::Jre,
    )
    .await
}

async fn run_java_installer(
    java: &Path,
    workdir: &str,
    jar: &Path,
    extra: &[&str],
) -> anyhow::Result<()> {
    let jar_name = jar
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid installer path"))?;
    let mut args = vec!["-jar".to_string(), jar_name.to_string()];
    args.extend(extra.iter().map(|s| s.to_string()));
    tracing::info!(dir = %workdir, jar = %jar_name, java = %java.display(), "running installer");
    let mut cmd = tokio::process::Command::new(java);
    crate::java::apply_java_home(&mut cmd, &java.to_string_lossy());
    crate::wincompat::hide_console(&mut cmd);
    cmd.current_dir(workdir)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(480), cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("installer timed out (java --installServer)"))?
        .map_err(|e| {
            anyhow::anyhow!("failed to spawn java (needed to run the installer): {e}")
        })?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        let tail = if err.trim().is_empty() { out } else { err };
        let tail = tail.chars().rev().take(1500).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        anyhow::bail!("installer failed: {tail}");
    }
    Ok(())
}

// --- Mohist / Banner ---

async fn list_mohist_project(project: &str) -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json(&format!("https://mohistmc.com/api/v2/projects/{project}")).await?;
    let ids = v
        .get("versions")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .filter(|id| !is_unstable(id))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, project))
}

async fn resolve_mohist_download_url(project: &str, version: &str) -> anyhow::Result<String> {
    let v = get_json(&format!(
        "https://mohistmc.com/api/v2/projects/{project}/{version}/builds"
    ))
    .await?;
    let builds = v
        .get("builds")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("{project} builds missing for {version}"))?;
    let pick = builds
        .iter()
        .max_by_key(|b| b.get("number").and_then(|n| n.as_u64()).unwrap_or(0))
        .ok_or_else(|| anyhow::anyhow!("no {project} build for {version}"))?;
    pick.get("url")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("{project} download url missing"))
}

// --- Arclight ---

const ARCLIGHT_ROOT: &str = "https://files.hypoglycemia.icu/v1/files";

fn arclight_file_url(key: &str) -> String {
    format!("{ARCLIGHT_ROOT}{key}")
}

async fn list_arclight_versions() -> anyhow::Result<Vec<CoreVersion>> {
    let v = get_json(&format!("{ARCLIGHT_ROOT}/arclight/minecraft")).await?;
    let ids = v
        .get("files")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.get("name").and_then(|x| x.as_str()))
        .filter(|id| !is_unstable(id))
        .map(|s| s.to_string())
        .collect();
    Ok(take_newest(ids, "arclight"))
}

async fn list_arclight_loaders(version: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let listing = get_json(&arclight_file_url(&format!(
        "/arclight/minecraft/{version}"
    )))
    .await?;
    let files = listing
        .get("files")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("arclight listing empty for {version}"))?;
    let stable = files.iter().find(|f| {
        f.get("name")
            .and_then(|x| x.as_str())
            .is_some_and(|n| n == "latest-stable")
    });
    let snap = files.iter().find(|f| {
        f.get("name")
            .and_then(|x| x.as_str())
            .is_some_and(|n| n == "latest-snapshot")
    });
    let pointer = stable.or(snap).ok_or_else(|| {
        anyhow::anyhow!("arclight latest-stable missing for {version}")
    })?;
    let key = pointer
        .get("key")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("arclight latest key missing"))?;
    let inner = get_json(&arclight_file_url(key)).await?;
    let mut out = Vec::new();
    if let Some(inner_files) = inner.get("files").and_then(|x| x.as_array()) {
        for f in inner_files {
            let Some(name) = f.get("name").and_then(|x| x.as_str()) else {
                continue;
            };
            let ty = f.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if ty != "object" && ty != "file" {
                continue;
            }
            out.push(CoreLoader {
                id: name.to_string(),
                latest: false,
                recommended: name.eq_ignore_ascii_case("neoforge"),
                label: None,
            });
        }
    }
    out.sort_by(|a, b| {
        let rank = |n: &str| match n.to_ascii_lowercase().as_str() {
            "neoforge" => 0,
            "forge" => 1,
            "fabric" => 2,
            _ => 3,
        };
        rank(&a.id).cmp(&rank(&b.id)).then(a.id.cmp(&b.id))
    });
    Ok(mark_latest_loaders(out))
}

async fn resolve_arclight_download_url(
    version: &str,
    loader: Option<&str>,
) -> anyhow::Result<String> {
    let listing = get_json(&arclight_file_url(&format!(
        "/arclight/minecraft/{version}"
    )))
    .await?;
    let files = listing
        .get("files")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("arclight listing empty for {version}"))?;
    let stable = files.iter().find(|f| {
        f.get("name")
            .and_then(|x| x.as_str())
            .is_some_and(|n| n == "latest-stable")
    });
    let snap = files.iter().find(|f| {
        f.get("name")
            .and_then(|x| x.as_str())
            .is_some_and(|n| n == "latest-snapshot")
    });
    let pointer = stable.or(snap).ok_or_else(|| {
        anyhow::anyhow!("arclight latest-stable missing for {version}")
    })?;
    let key = pointer
        .get("key")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("arclight latest key missing"))?;
    let inner = get_json(&arclight_file_url(key)).await?;
    let inner_files = inner
        .get("files")
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow::anyhow!("arclight artifacts missing"))?;
    let mut prefer: Vec<&str> = vec!["neoforge", "forge", "fabric"];
    if let Some(want) = loader {
        prefer.retain(|p| !p.eq_ignore_ascii_case(want));
        prefer.insert(0, want);
    }
    let art = prefer
        .iter()
        .find_map(|want| {
            inner_files.iter().find(|f| {
                f.get("name")
                    .and_then(|x| x.as_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(want))
            })
        })
        .or(inner_files.iter().find(|f| {
            f.get("type")
                .and_then(|x| x.as_str())
                .is_some_and(|t| t == "object")
        }))
        .ok_or_else(|| anyhow::anyhow!("no arclight jar for {version}"))?;
    let art_key = art
        .get("key")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("arclight artifact key missing"))?;
    Ok(arclight_file_url(art_key))
}
