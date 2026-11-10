use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const STAGE_ORDER: &[&str] = &[
    "DT", "DP", "AT-D", "AT", "BT-D", "BT", "RC-D", "RC", "GA", "HF", "LTS",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CocktailVersion {
    pub year: u32,
    pub quarter: u32,
    pub major: u32,
    pub stage: String,
    pub stage_rank: usize,
    pub build: u32,
}

impl CocktailVersion {
    pub fn stage_rank(stage: &str) -> usize {
        STAGE_ORDER
            .iter()
            .position(|s| s.eq_ignore_ascii_case(stage))
            .unwrap_or(0)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let text = raw.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let (body, build) = match text.split_once('+') {
            Some((b, n)) => {
                let clean = n.trim_start_matches('B').trim_start_matches('b');
                (b, clean.parse::<u32>().unwrap_or(0))
            }
            None => (text, 0),
        };
        let parts: Vec<&str> = body.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        let head = parts[0];
        let (year, quarter) = if let Some(qpos) = head.find(['Q', 'q']) {
            let year = head[..qpos].parse::<u32>().ok()?;
            let quarter = head[qpos + 1..].parse::<u32>().ok()?;
            (year, quarter)
        } else {
            let major = head.parse::<u32>().ok()?;
            let quarter = parts.get(1)?.parse::<u32>().ok()?;
            (major, quarter)
        };
        let major = parts.get(1)?.parse::<u32>().ok()?;
        let stage_raw = parts.get(2).copied().unwrap_or("DT");
        let stage = stage_raw.to_ascii_uppercase();
        Some(Self {
            year,
            quarter,
            major,
            stage_rank: Self::stage_rank(&stage),
            stage,
            build,
        })
    }

    pub fn is_newer_than(&self, other: &Self) -> bool {
        (self.year, self.quarter, self.major, self.stage_rank, self.build)
            > (other.year, other.quarter, other.major, other.stage_rank, other.build)
    }

    pub fn is_stable(&self) -> bool {
        self.stage == "GA" || self.stage == "HF" || self.stage == "LTS"
    }

    pub fn display(&self) -> String {
        let base = format!("{}Q{}.{}.{}", self.year, self.quarter, self.major, self.stage);
        if self.build > 0 {
            format!("{base}+B{}", self.build)
        } else {
            base
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    pub download_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag: String,
    pub name: String,
    pub body: String,
    pub published_at: String,
    pub prerelease: bool,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    pub latest_is_stable: bool,
    pub checked_at: String,
    pub source: String,
    pub notes_url: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlatformPick {
    pub platform: String,
    pub asset_name: Option<String>,
    pub download_url: Option<String>,
    pub size: u64,
}

pub fn pick_asset(release: &ReleaseInfo, platform: &str) -> PlatformPick {
    let mut pick = PlatformPick {
        platform: platform.to_string(),
        ..Default::default()
    };
    let want: Vec<&str> = match platform {
        "linux-x64-deb" => vec![".deb", "amd64"],
        "linux-x64-rpm" => vec![".rpm", "x86_64"],
        "linux-arm64-deb" => vec![".deb", "arm64"],
        "windows-x64-zip" => vec!["windows", ".zip"],
        "windows-x64-msi" => vec!["windows", ".msi"],
        _ => vec![],
    };
    if want.is_empty() {
        return pick;
    }
    for asset in &release.assets {
        let lower = asset.name.to_ascii_lowercase();
        if want.iter().all(|w| lower.contains(w)) {
            pick.asset_name = Some(asset.name.clone());
            pick.download_url = Some(asset.download_url.clone());
            pick.size = asset.size;
            return pick;
        }
    }
    pick
}

#[derive(Clone, Debug)]
pub struct RepoConfig {
    pub owner: String,
    pub repo: String,
    pub ttl: Duration,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            owner: "CocktailMC".into(),
            repo: "Cocktail".into(),
            ttl: Duration::from_secs(6 * 3600),
        }
    }
}

impl RepoConfig {
    pub fn api_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases?per_page=20",
            self.owner, self.repo
        )
    }

    pub fn page_url(&self) -> String {
        format!("https://github.com/{}/{}/releases", self.owner, self.repo)
    }
}

pub struct ReleaseCache {
    inner: Mutex<Option<(Instant, Vec<ReleaseInfo>)>>,
}

impl ReleaseCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn fresh(&self, ttl: Duration) -> Option<Vec<ReleaseInfo>> {
        let guard = self.inner.lock().ok()?;
        match guard.as_ref() {
            Some((at, list)) if at.elapsed() < ttl => Some(list.clone()),
            _ => None,
        }
    }

    pub fn store(&self, list: Vec<ReleaseInfo>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((Instant::now(), list));
        }
    }

    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

impl Default for ReleaseCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_releases(json: &str) -> Option<Vec<ReleaseInfo>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = value.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let tag = item.get("tag_name")?.as_str()?.to_string();
        let assets = item
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|a| {
                        Some(ReleaseAsset {
                            name: a.get("name")?.as_str()?.to_string(),
                            size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                            download_url: a
                                .get("browser_download_url")
                                .and_then(|u| u.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(ReleaseInfo {
            tag,
            name: item
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
            body: item
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string(),
            published_at: item
                .get("published_at")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string(),
            prerelease: item
                .get("prerelease")
                .and_then(|p| p.as_bool())
                .unwrap_or(false),
            html_url: item
                .get("html_url")
                .and_then(|h| h.as_str())
                .unwrap_or("")
                .to_string(),
            assets,
        });
    }
    Some(out)
}

pub fn newest(releases: &[ReleaseInfo], include_prerelease: bool) -> Option<(&ReleaseInfo, CocktailVersion)> {
    let mut best: Option<(&ReleaseInfo, CocktailVersion)> = None;
    for rel in releases {
        if rel.prerelease && !include_prerelease {
            continue;
        }
        let Some(ver) = CocktailVersion::parse(&rel.tag) else {
            continue;
        };
        match &best {
            Some((_, cur)) if !ver.is_newer_than(cur) => {}
            _ => best = Some((rel, ver)),
        }
    }
    best
}

pub fn evaluate(
    current_raw: &str,
    releases: &[ReleaseInfo],
    include_prerelease: bool,
    source: &str,
) -> UpdateStatus {
    let now = chrono::Utc::now().to_rfc3339();
    let current = CocktailVersion::parse(current_raw);
    let latest = newest(releases, include_prerelease);
    match (current, latest) {
        (Some(cur), Some((rel, ver))) => UpdateStatus {
            current: cur.display(),
            latest: Some(ver.display()),
            update_available: ver.is_newer_than(&cur),
            latest_is_stable: ver.is_stable(),
            checked_at: now,
            source: source.to_string(),
            notes_url: rel.html_url.clone(),
            error: None,
        },
        (None, _) => UpdateStatus {
            current: current_raw.to_string(),
            latest: latest.map(|(_, v)| v.display()),
            update_available: false,
            latest_is_stable: false,
            checked_at: now,
            source: source.to_string(),
            notes_url: String::new(),
            error: Some("当前版本号无法解析".into()),
        },
        (_, None) => UpdateStatus {
            current: current_raw.to_string(),
            latest: None,
            update_available: false,
            latest_is_stable: false,
            checked_at: now,
            source: source.to_string(),
            notes_url: String::new(),
            error: Some("没有可解析的发布版本".into()),
        },
    }
}

pub fn channel_filter(releases: &[ReleaseInfo], channel: &str) -> Vec<ReleaseInfo> {
    match channel {
        "stable" => releases
            .iter()
            .filter(|r| {
                CocktailVersion::parse(&r.tag)
                    .map(|v| v.is_stable())
                    .unwrap_or(false)
            })
            .cloned()
            .collect(),
        "beta" => releases.to_vec(),
        _ => releases.clone(),
    }
}

pub fn summarize_channels(releases: &[ReleaseInfo]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for rel in releases {
        let key = CocktailVersion::parse(&rel.tag)
            .map(|v| v.stage)
            .unwrap_or_else(|| "unknown".into());
        *out.entry(key).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cocktail_version_formats() {
        let v = CocktailVersion::parse("26Q4.11.DP").unwrap();
        assert_eq!((v.year, v.quarter, v.major, v.build), (26, 4, 11, 0));
        assert_eq!(v.stage, "DP");
        let with_build = CocktailVersion::parse("v26Q4.11.GA+B1842").unwrap();
        assert_eq!(with_build.stage, "GA");
        assert_eq!(with_build.build, 1842);
        let semver = CocktailVersion::parse("26.4.11-DP").unwrap();
        assert_eq!((semver.year, semver.quarter, semver.major), (26, 4, 11));
    }

    #[test]
    fn rejects_garbage_versions() {
        assert!(CocktailVersion::parse("").is_none());
        assert!(CocktailVersion::parse("abc").is_none());
        assert!(CocktailVersion::parse("26").is_none());
    }

    #[test]
    fn stage_ordering() {
        let dt = CocktailVersion::parse("26Q4.11.DT").unwrap();
        let ga = CocktailVersion::parse("26Q4.11.GA").unwrap();
        assert!(ga.is_newer_than(&dt));
        assert!(ga.is_stable());
        assert!(!dt.is_stable());
    }

    #[test]
    fn build_number_breaks_ties() {
        let a = CocktailVersion::parse("26Q4.11.GA+B100").unwrap();
        let b = CocktailVersion::parse("26Q4.11.GA+B200").unwrap();
        assert!(b.is_newer_than(&a));
        assert!(!a.is_newer_than(&b));
    }

    #[test]
    fn display_roundtrip() {
        let v = CocktailVersion::parse("26Q4.11.DP+B42").unwrap();
        assert_eq!(v.display(), "26Q4.11.DP+B42");
    }

    #[test]
    fn parse_releases_from_github_json() {
        let json = r#"[
          {"tag_name":"26Q4.11.GA","name":"GA","body":"notes","published_at":"2026-11-01T00:00:00Z","prerelease":false,"html_url":"https://x/1",
           "assets":[{"name":"cocktail_26Q4.11_amd64.deb","size":1024,"browser_download_url":"https://x/a.deb"}]},
          {"tag_name":"26Q4.12.DP","name":"DP","body":"","published_at":"2026-11-05T00:00:00Z","prerelease":true,"html_url":"https://x/2","assets":[]}
        ]"#;
        let list = parse_releases(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].assets.len(), 1);
        assert_eq!(list[0].assets[0].size, 1024);
        assert!(list[1].prerelease);
    }

    #[test]
    fn newest_respects_channel() {
        let json = r#"[
          {"tag_name":"26Q4.11.GA","name":"","body":"","published_at":"","prerelease":false,"html_url":"","assets":[]},
          {"tag_name":"26Q4.12.DP","name":"","body":"","published_at":"","prerelease":true,"html_url":"","assets":[]}
        ]"#;
        let list = parse_releases(json).unwrap();
        let stable = newest(&list, false).unwrap().1;
        assert_eq!(stable.stage, "GA");
        let any = newest(&list, true).unwrap().1;
        assert_eq!(any.major, 12);
    }

    #[test]
    fn evaluate_detects_update() {
        let json = r#"[
          {"tag_name":"26Q4.30.GA","name":"","body":"","published_at":"","prerelease":false,"html_url":"https://x/new","assets":[]}
        ]"#;
        let list = parse_releases(json).unwrap();
        let st = evaluate("26Q4.11.DP", &list, false, "github");
        assert!(st.update_available);
        assert_eq!(st.latest.as_deref(), Some("26Q4.30.GA"));
        assert!(st.latest_is_stable);
        assert_eq!(st.notes_url, "https://x/new");
    }

    #[test]
    fn evaluate_no_update_when_current_newer() {
        let json = r#"[
          {"tag_name":"26Q4.5.GA","name":"","body":"","published_at":"","prerelease":false,"html_url":"","assets":[]}
        ]"#;
        let list = parse_releases(json).unwrap();
        let st = evaluate("26Q4.11.DP", &list, false, "github");
        assert!(!st.update_available);
    }

    #[test]
    fn evaluate_reports_errors() {
        let st = evaluate("garbage", &[], false, "github");
        assert!(!st.update_available);
        assert!(st.error.is_some());
        assert_eq!(st.current, "garbage");
    }

    #[test]
    fn asset_picking_by_platform() {
        let rel = ReleaseInfo {
            tag: "26Q4.11.GA".into(),
            name: String::new(),
            body: String::new(),
            published_at: String::new(),
            prerelease: false,
            html_url: String::new(),
            assets: vec![
                ReleaseAsset { name: "cocktail_26Q4.11_amd64.deb".into(), size: 10, download_url: "https://x/deb".into() },
                ReleaseAsset { name: "cocktail-26Q4.11-windows-x64.zip".into(), size: 20, download_url: "https://x/zip".into() },
                ReleaseAsset { name: "cocktail-26Q4.11-windows-x64.msi".into(), size: 30, download_url: "https://x/msi".into() },
            ],
        };
        assert_eq!(pick_asset(&rel, "linux-x64-deb").download_url.as_deref(), Some("https://x/deb"));
        assert_eq!(pick_asset(&rel, "windows-x64-msi").size, 30);
        assert!(pick_asset(&rel, "linux-arm64-deb").asset_name.is_none());
    }

    #[test]
    fn cache_ttl_behaviour() {
        let cache = ReleaseCache::new();
        assert!(cache.fresh(Duration::from_secs(60)).is_none());
        cache.store(vec![]);
        assert!(cache.fresh(Duration::from_secs(60)).is_some());
        assert!(cache.fresh(Duration::from_secs(0)).is_none());
        cache.invalidate();
        assert!(cache.fresh(Duration::from_secs(60)).is_none());
    }

    #[test]
    fn channel_filtering_and_summary() {
        let json = r#"[
          {"tag_name":"26Q4.11.GA","name":"","body":"","published_at":"","prerelease":false,"html_url":"","assets":[]},
          {"tag_name":"26Q4.12.DP","name":"","body":"","published_at":"","prerelease":true,"html_url":"","assets":[]}
        ]"#;
        let list = parse_releases(json).unwrap();
        assert_eq!(channel_filter(&list, "stable").len(), 1);
        assert_eq!(channel_filter(&list, "beta").len(), 2);
        let summary = summarize_channels(&list);
        assert_eq!(summary.get("GA"), Some(&1));
        assert_eq!(summary.get("DP"), Some(&1));
    }

    #[test]
    fn repo_config_urls() {
        let cfg = RepoConfig::default();
        assert!(cfg.api_url().contains("CocktailMC/Cocktail"));
        assert!(cfg.page_url().ends_with("/releases"));
    }
}
