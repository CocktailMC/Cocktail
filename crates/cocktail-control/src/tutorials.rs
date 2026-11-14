use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tutorial {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub source: VideoSource,
    pub duration_secs: u32,
    pub chapter: String,
    pub order: u32,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VideoSource {
    Local { path: String },
    External { url: String, poster: Option<String> },
    Youtube { id: String },
    Bilibili { bvid: String },
}

impl VideoSource {
    pub fn playable_url(&self) -> Option<String> {
        match self {
            VideoSource::Local { path } => Some(path.clone()),
            VideoSource::External { url, .. } => Some(url.clone()),
            VideoSource::Youtube { id } => Some(format!("https://www.youtube.com/embed/{id}")),
            VideoSource::Bilibili { bvid } => Some(format!(
                "https://player.bilibili.com/player.html?bvid={bvid}"
            )),
        }
    }

    pub fn is_embedded(&self) -> bool {
        matches!(
            self,
            VideoSource::Youtube { .. } | VideoSource::Bilibili { .. }
        )
    }

    pub fn is_local(&self) -> bool {
        matches!(self, VideoSource::Local { .. })
    }

    pub fn is_safe(&self) -> bool {
        match self {
            VideoSource::Local { path } => {
                !path.contains("..") && (path.starts_with('/') || !path.contains(':'))
            }
            VideoSource::External { url, .. } => {
                url.starts_with("https://") || url.starts_with("/")
            }
            VideoSource::Youtube { id } => {
                !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
            VideoSource::Bilibili { bvid } => {
                bvid.starts_with("BV") && bvid.chars().all(|c| c.is_ascii_alphanumeric())
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Progress {
    pub watched_secs: u32,
    pub completed: bool,
    pub updated_at: String,
}

impl Progress {
    pub fn percent(&self, duration: u32) -> u32 {
        if duration == 0 {
            return if self.completed { 100 } else { 0 };
        }
        ((self.watched_secs.min(duration) as f64 / duration as f64) * 100.0).round() as u32
    }

    pub fn is_complete(&self, duration: u32) -> bool {
        if self.completed {
            return true;
        }
        duration > 0 && self.watched_secs * 100 >= duration * 95
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TutorialView {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub chapter: String,
    pub order: u32,
    pub duration_secs: u32,
    pub url: Option<String>,
    pub embedded: bool,
    pub local: bool,
    pub tags: Vec<String>,
    pub progress: Progress,
    pub percent: u32,
    pub completed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub tutorials: Vec<Tutorial>,
}

impl Catalog {
    pub fn defaults() -> Self {
        let mk = |id: &str,
                  title: &str,
                  summary: &str,
                  chapter: &str,
                  order: u32,
                  duration: u32,
                  source: VideoSource,
                  tags: &[&str]| Tutorial {
            id: id.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            source,
            duration_secs: duration,
            chapter: chapter.to_string(),
            order,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        };
        Self {
            tutorials: vec![
                mk(
                    "install-linux",
                    "Linux 安装到开服",
                    "从 deb 包安装、首次初始化到启动第一个实例的完整演示",
                    "安装",
                    1,
                    240,
                    VideoSource::Local {
                        path: "/tutorials/install-linux.mp4".into(),
                    },
                    &["安装", "入门"],
                ),
                mk(
                    "install-windows",
                    "Windows 便携包启动",
                    "解压即用，双击脚本启动控制面并在浏览器完成初始化",
                    "安装",
                    2,
                    150,
                    VideoSource::Local {
                        path: "/tutorials/install-windows.mp4".into(),
                    },
                    &["安装", "Windows"],
                ),
                mk(
                    "first-instance",
                    "创建第一个服务器",
                    "选择核心版本、配置内存与端口、启动并通过 RCON 验证",
                    "入门",
                    3,
                    300,
                    VideoSource::Local {
                        path: "/tutorials/first-instance.mp4".into(),
                    },
                    &["入门", "实例"],
                ),
                mk(
                    "backup-restore",
                    "备份与恢复",
                    "增量备份、GFS 保留策略与异常后的恢复流程",
                    "运维",
                    4,
                    270,
                    VideoSource::Local {
                        path: "/tutorials/backup-restore.mp4".into(),
                    },
                    &["运维", "备份"],
                ),
                mk(
                    "permissions",
                    "多人协作与权限",
                    "角色、按实例授权与临时授权的使用场景",
                    "管理",
                    5,
                    210,
                    VideoSource::Local {
                        path: "/tutorials/permissions.mp4".into(),
                    },
                    &["管理", "权限"],
                ),
            ],
        }
    }

    pub fn by_chapter(&self) -> BTreeMap<String, Vec<&Tutorial>> {
        let mut out: BTreeMap<String, Vec<&Tutorial>> = BTreeMap::new();
        for t in &self.tutorials {
            out.entry(t.chapter.clone()).or_default().push(t);
        }
        for list in out.values_mut() {
            list.sort_by_key(|t| t.order);
        }
        out
    }

    pub fn find(&self, id: &str) -> Option<&Tutorial> {
        self.tutorials.iter().find(|t| t.id == id)
    }

    pub fn total_duration(&self) -> u32 {
        self.tutorials.iter().map(|t| t.duration_secs).sum()
    }

    pub fn unsafe_entries(&self) -> Vec<String> {
        self.tutorials
            .iter()
            .filter(|t| !t.source.is_safe())
            .map(|t| t.id.clone())
            .collect()
    }

    pub fn search(&self, needle: &str) -> Vec<&Tutorial> {
        let q = needle.trim().to_lowercase();
        if q.is_empty() {
            return self.tutorials.iter().collect();
        }
        self.tutorials
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&q)
                    || t.summary.to_lowercase().contains(&q)
                    || t.chapter.to_lowercase().contains(&q)
                    || t.tags.iter().any(|g| g.to_lowercase().contains(&q))
            })
            .collect()
    }
}

pub fn build_views(catalog: &Catalog, progress: &BTreeMap<String, Progress>) -> Vec<TutorialView> {
    let mut out: Vec<TutorialView> = catalog
        .tutorials
        .iter()
        .map(|t| {
            let p = progress.get(&t.id).cloned().unwrap_or_default();
            TutorialView {
                id: t.id.clone(),
                title: t.title.clone(),
                summary: t.summary.clone(),
                chapter: t.chapter.clone(),
                order: t.order,
                duration_secs: t.duration_secs,
                url: t.source.playable_url(),
                embedded: t.source.is_embedded(),
                local: t.source.is_local(),
                tags: t.tags.clone(),
                percent: p.percent(t.duration_secs),
                completed: p.is_complete(t.duration_secs),
                progress: p,
            }
        })
        .collect();
    out.sort_by(|a, b| a.chapter.cmp(&b.chapter).then(a.order.cmp(&b.order)));
    out
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WatchSummary {
    pub total: usize,
    pub completed: usize,
    pub percent: u32,
    pub next: Option<String>,
}

pub fn summarize(views: &[TutorialView]) -> WatchSummary {
    if views.is_empty() {
        return WatchSummary::default();
    }
    let completed = views.iter().filter(|v| v.completed).count();
    let next = views.iter().find(|v| !v.completed).map(|v| v.id.clone());
    WatchSummary {
        total: views.len(),
        completed,
        percent: ((completed as f64 / views.len() as f64) * 100.0).round() as u32,
        next,
    }
}

pub fn apply_progress(
    progress: &mut BTreeMap<String, Progress>,
    id: &str,
    watched_secs: u32,
    duration: u32,
) -> Progress {
    let entry = progress.entry(id.to_string()).or_default();
    if watched_secs > entry.watched_secs {
        entry.watched_secs = watched_secs;
    }
    if duration > 0 && entry.watched_secs * 100 >= duration * 95 {
        entry.completed = true;
        entry.watched_secs = duration;
    }
    entry.updated_at = chrono::Utc::now().to_rfc3339();
    entry.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_well_formed() {
        let c = Catalog::defaults();
        assert!(c.tutorials.len() >= 5);
        assert!(c.unsafe_entries().is_empty());
        assert!(c.total_duration() > 0);
        assert!(c.find("install-linux").is_some());
        assert!(c.find("nope").is_none());
    }

    #[test]
    fn chapters_grouped_and_ordered() {
        let c = Catalog::defaults();
        let groups = c.by_chapter();
        assert!(groups.contains_key("安装"));
        let install = groups.get("安装").unwrap();
        assert_eq!(install[0].order, 1);
        assert_eq!(install[1].order, 2);
    }

    #[test]
    fn video_source_safety_checks() {
        assert!(
            VideoSource::Local {
                path: "/tutorials/a.mp4".into()
            }
            .is_safe()
        );
        assert!(
            !VideoSource::Local {
                path: "/tutorials/../../etc/passwd".into()
            }
            .is_safe()
        );
        assert!(
            VideoSource::External {
                url: "https://cdn.example/a.mp4".into(),
                poster: None
            }
            .is_safe()
        );
        assert!(
            !VideoSource::External {
                url: "http://insecure/a.mp4".into(),
                poster: None
            }
            .is_safe()
        );
        assert!(
            VideoSource::Youtube {
                id: "dQw4w9WgXcQ".into()
            }
            .is_safe()
        );
        assert!(
            !VideoSource::Youtube {
                id: "../bad".into()
            }
            .is_safe()
        );
        assert!(
            VideoSource::Bilibili {
                bvid: "BV1xx411c7mD".into()
            }
            .is_safe()
        );
        assert!(
            !VideoSource::Bilibili {
                bvid: "notbvid".into()
            }
            .is_safe()
        );
    }

    #[test]
    fn playable_urls() {
        assert_eq!(
            VideoSource::Local {
                path: "/t/a.mp4".into()
            }
            .playable_url()
            .as_deref(),
            Some("/t/a.mp4")
        );
        let yt = VideoSource::Youtube { id: "abc".into() };
        assert!(yt.playable_url().unwrap().contains("youtube.com/embed/abc"));
        assert!(yt.is_embedded());
        assert!(!yt.is_local());
    }

    #[test]
    fn progress_percent_and_completion() {
        let mut p = Progress::default();
        assert_eq!(p.percent(100), 0);
        p.watched_secs = 50;
        assert_eq!(p.percent(100), 50);
        assert!(!p.is_complete(100));
        p.watched_secs = 95;
        assert!(p.is_complete(100));
        assert_eq!(p.percent(100), 95);
    }

    #[test]
    fn apply_progress_monotonic() {
        let mut map = BTreeMap::new();
        apply_progress(&mut map, "a", 30, 100);
        assert_eq!(map.get("a").unwrap().watched_secs, 30);
        apply_progress(&mut map, "a", 10, 100);
        assert_eq!(map.get("a").unwrap().watched_secs, 30);
        let p = apply_progress(&mut map, "a", 96, 100);
        assert!(p.completed);
        assert_eq!(p.watched_secs, 100);
    }

    #[test]
    fn views_merge_progress() {
        let c = Catalog::defaults();
        let mut map = BTreeMap::new();
        apply_progress(&mut map, "install-linux", 240, 240);
        let views = build_views(&c, &map);
        let first = views.iter().find(|v| v.id == "install-linux").unwrap();
        assert!(first.completed);
        assert_eq!(first.percent, 100);
        assert!(first.local);
    }

    #[test]
    fn summary_reports_next() {
        let c = Catalog::defaults();
        let mut map = BTreeMap::new();
        apply_progress(&mut map, "install-linux", 240, 240);
        let views = build_views(&c, &map);
        let s = summarize(&views);
        assert_eq!(s.completed, 1);
        assert!(s.percent > 0);
        assert!(s.next.is_some());
        assert_ne!(s.next.as_deref(), Some("install-linux"));
    }

    #[test]
    fn search_matches_title_tags_chapter() {
        let c = Catalog::defaults();
        assert_eq!(c.search("备份").len(), 1);
        assert!(!c.search("Windows").is_empty());
        assert_eq!(c.search("权限").len(), 1);
        assert_eq!(c.search("").len(), c.tutorials.len());
        assert!(c.search("zzz-none").is_empty());
    }

    #[test]
    fn empty_catalog_summary() {
        let s = summarize(&[]);
        assert_eq!(s.total, 0);
        assert_eq!(s.percent, 0);
        assert!(s.next.is_none());
    }
}
