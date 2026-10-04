use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let core = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
        let mut parts = core.split('.');
        let major = parts.next()?.trim().parse().ok()?;
        let minor = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
        let patch = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn satisfies(&self, constraint: &str) -> bool {
        let expr = constraint.trim();
        if expr.is_empty() || expr == "*" {
            return true;
        }
        for clause in expr.split(',') {
            let clause = clause.trim();
            if clause.is_empty() {
                continue;
            }
            if let Some(rest) = clause.strip_prefix(">=") {
                if let Some(v) = Version::parse(rest) {
                    if self < &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(rest) = clause.strip_prefix("<=") {
                if let Some(v) = Version::parse(rest) {
                    if self > &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(rest) = clause.strip_prefix('>') {
                if let Some(v) = Version::parse(rest) {
                    if self <= &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(rest) = clause.strip_prefix('<') {
                if let Some(v) = Version::parse(rest) {
                    if self >= &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(rest) = clause.strip_prefix('^') {
                if let Some(v) = Version::parse(rest) {
                    if self.major != v.major || self < &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(rest) = clause.strip_prefix('~') {
                if let Some(v) = Version::parse(rest) {
                    if self.major != v.major || self.minor != v.minor || self < &v {
                        return false;
                    }
                    continue;
                }
            }
            if let Some(v) = Version::parse(clause) {
                if self != &v {
                    return false;
                }
                continue;
            }
            return false;
        }
        true
    }

    pub fn bump(&self, kind: &str) -> Self {
        match kind {
            "major" => Self {
                major: self.major + 1,
                minor: 0,
                patch: 0,
            },
            "minor" => Self {
                major: self.major,
                minor: self.minor + 1,
                patch: 0,
            },
            _ => Self {
                major: self.major,
                minor: self.minor,
                patch: self.patch + 1,
            },
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    pub constraint: String,
    pub optional: bool,
    pub kind: String,
}

impl Dependency {
    pub fn required(id: &str, constraint: &str) -> Self {
        Self {
            id: id.to_string(),
            constraint: constraint.to_string(),
            optional: false,
            kind: "required".into(),
        }
    }

    pub fn optional(id: &str, constraint: &str) -> Self {
        Self {
            id: id.to_string(),
            constraint: constraint.to_string(),
            optional: true,
            kind: "optional".into(),
        }
    }

    pub fn conflicts(id: &str, constraint: &str) -> Self {
        Self {
            id: id.to_string(),
            constraint: constraint.to_string(),
            optional: false,
            kind: "conflict".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
    pub mc_range: String,
    pub depends: Vec<Dependency>,
    pub provides: Vec<String>,
    pub side: String,
    pub java_major: Option<u32>,
}

impl PluginManifest {
    pub fn new(id: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            loader: "paper".into(),
            mc_range: String::new(),
            depends: Vec::new(),
            provides: Vec::new(),
            side: "both".into(),
            java_major: None,
        }
    }

    pub fn with_loader(mut self, loader: &str) -> Self {
        self.loader = loader.to_string();
        self
    }

    pub fn with_mc(mut self, range: &str) -> Self {
        self.mc_range = range.to_string();
        self
    }

    pub fn with_dep(mut self, dep: Dependency) -> Self {
        self.depends.push(dep);
        self
    }

    pub fn version_parsed(&self) -> Option<Version> {
        Version::parse(&self.version)
    }

    pub fn declares(&self, id: &str) -> bool {
        self.id == id || self.provides.iter().any(|p| p == id)
    }

    pub fn matches_mc(&self, mc: &str) -> bool {
        if self.mc_range.is_empty() {
            return true;
        }
        Version::parse(mc)
            .map(|v| v.satisfies(&self.mc_range))
            .unwrap_or(true)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum IssueKind {
    MissingDependency,
    VersionConflict,
    LoaderMismatch,
    McVersionMismatch,
    JavaTooOld,
    DuplicatePlugin,
    ExplicitConflict,
    DependencyCycle,
    SideMismatch,
}

impl IssueKind {
    pub fn severity(&self) -> &'static str {
        match self {
            IssueKind::MissingDependency
            | IssueKind::VersionConflict
            | IssueKind::DuplicatePlugin
            | IssueKind::ExplicitConflict
            | IssueKind::DependencyCycle
            | IssueKind::LoaderMismatch
            | IssueKind::McVersionMismatch
            | IssueKind::JavaTooOld => "error",
            _ => "warning",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            IssueKind::MissingDependency => "缺少依赖",
            IssueKind::VersionConflict => "版本冲突",
            IssueKind::LoaderMismatch => "加载器不匹配",
            IssueKind::McVersionMismatch => "游戏版本不兼容",
            IssueKind::JavaTooOld => "Java 版本过低",
            IssueKind::DuplicatePlugin => "重复插件",
            IssueKind::ExplicitConflict => "声明冲突",
            IssueKind::DependencyCycle => "依赖成环",
            IssueKind::SideMismatch => "端侧不匹配",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub kind: IssueKind,
    pub severity: String,
    pub plugin: String,
    pub message: String,
    pub fix: String,
}

impl Issue {
    fn new(kind: IssueKind, plugin: &str, message: &str, fix: &str) -> Self {
        Self {
            severity: kind.severity().to_string(),
            kind,
            plugin: plugin.to_string(),
            message: message.to_string(),
            fix: fix.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resolution {
    pub install_order: Vec<String>,
    pub issues: Vec<Issue>,
    pub missing: Vec<String>,
    pub ok: bool,
}

impl Resolution {
    pub fn errors(&self) -> Vec<&Issue> {
        self.issues.iter().filter(|i| i.severity == "error").collect()
    }

    pub fn warnings(&self) -> Vec<&Issue> {
        self.issues.iter().filter(|i| i.severity == "warning").collect()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} 个插件, {} 个错误, {} 个警告, 安装顺序 {} 步",
            self.install_order.len(),
            self.errors().len(),
            self.warnings().len(),
            self.install_order.len()
        )
    }
}

pub struct Resolver<'a> {
    manifests: &'a [PluginManifest],
    mc_version: String,
    loader: String,
    java_major: u32,
    client_only_ok: bool,
}

impl<'a> Resolver<'a> {
    pub fn new(manifests: &'a [PluginManifest]) -> Self {
        Self {
            manifests,
            mc_version: String::new(),
            loader: String::new(),
            java_major: 0,
            client_only_ok: false,
        }
    }

    pub fn for_target(mut self, mc: &str, loader: &str, java: u32) -> Self {
        self.mc_version = mc.to_string();
        self.loader = loader.to_string();
        self.java_major = java;
        self
    }

    pub fn allow_client_only(mut self, allow: bool) -> Self {
        self.client_only_ok = allow;
        self
    }

    fn provider(&self, id: &str) -> Option<&PluginManifest> {
        self.manifests.iter().find(|m| m.declares(id))
    }

    pub fn resolve(&self) -> Resolution {
        let mut issues = Vec::new();
        let mut missing = Vec::new();
        let mut seen_ids: BTreeMap<&str, &PluginManifest> = BTreeMap::new();
        for m in self.manifests {
            if let Some(prev) = seen_ids.get(m.id.as_str()) {
                issues.push(Issue::new(
                    IssueKind::DuplicatePlugin,
                    &m.id,
                    &format!("与 {} 重复", prev.version),
                    "只保留一个版本",
                ));
            } else {
                seen_ids.insert(m.id.as_str(), m);
            }
        }
        for m in self.manifests {
            if !self.loader.is_empty() && m.loader != self.loader && m.loader != "any" {
                issues.push(Issue::new(
                    IssueKind::LoaderMismatch,
                    &m.id,
                    &format!("插件需要 {} 但目标是 {}", m.loader, self.loader),
                    "更换匹配加载器的构建",
                ));
            }
            if !self.mc_version.is_empty() && !m.matches_mc(&self.mc_version) {
                issues.push(Issue::new(
                    IssueKind::McVersionMismatch,
                    &m.id,
                    &format!("支持 {} 但服务器是 {}", m.mc_range, self.mc_version),
                    "升级到支持该版本的构建",
                ));
            }
            if let (Some(need), true) = (m.java_major, self.java_major > 0) {
                if need > self.java_major {
                    issues.push(Issue::new(
                        IssueKind::JavaTooOld,
                        &m.id,
                        &format!("需要 Java {need} 但当前 {}", self.java_major),
                        "切换实例 Java 大版本",
                    ));
                }
            }
            if !self.client_only_ok && m.side == "client" {
                issues.push(Issue::new(
                    IssueKind::SideMismatch,
                    &m.id,
                    "该插件仅限客户端",
                    "服务端无需安装",
                ));
            }
            for dep in &m.depends {
                match dep.kind.as_str() {
                    "conflict" => {
                        if self.provider(&dep.id).is_some() {
                            issues.push(Issue::new(
                                IssueKind::ExplicitConflict,
                                &m.id,
                                &format!("与 {} 声明冲突", dep.id),
                                "二选一移除",
                            ));
                        }
                    }
                    _ => {
                        match self.provider(&dep.id) {
                            None => {
                                if !dep.optional {
                                    missing.push(dep.id.clone());
                                    issues.push(Issue::new(
                                        IssueKind::MissingDependency,
                                        &m.id,
                                        &format!("缺少 {}", dep.id),
                                        &format!("安装 {} {}", dep.id, dep.constraint),
                                    ));
                                }
                            }
                            Some(p) => {
                                let ok = p
                                    .version_parsed()
                                    .map(|v| v.satisfies(&dep.constraint))
                                    .unwrap_or(true);
                                if !ok {
                                    issues.push(Issue::new(
                                        IssueKind::VersionConflict,
                                        &m.id,
                                        &format!(
                                            "需要 {} {} 但已装 {}",
                                            dep.id, dep.constraint, p.version
                                        ),
                                        &format!("升级 {} 到满足 {}", dep.id, dep.constraint),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        let order = self.install_order();
        if order.len() < self.manifests.len() {
            let known: BTreeSet<&str> = order.iter().map(|s| s.as_str()).collect();
            for m in self.manifests {
                if !known.contains(m.id.as_str()) {
                    issues.push(Issue::new(
                        IssueKind::DependencyCycle,
                        &m.id,
                        "依赖关系成环, 无法确定安装顺序",
                        "打破循环依赖",
                    ));
                }
            }
        }
        missing.sort();
        missing.dedup();
        let ok = !issues.iter().any(|i| i.severity == "error");
        Resolution {
            install_order: order,
            issues,
            missing,
            ok,
        }
    }

    pub fn install_order(&self) -> Vec<String> {
        let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
        let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for m in self.manifests {
            indegree.entry(m.id.clone()).or_insert(0);
        }
        for m in self.manifests {
            for dep in &m.depends {
                if dep.kind == "conflict" {
                    continue;
                }
                if let Some(p) = self.provider(&dep.id) {
                    if p.id == m.id {
                        continue;
                    }
                    adj.entry(p.id.clone()).or_default().push(m.id.clone());
                    *indegree.entry(m.id.clone()).or_insert(0) += 1;
                }
            }
        }
        let mut queue: VecDeque<String> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut sorted: Vec<String> = queue.drain(..).collect();
        sorted.sort();
        let mut queue: VecDeque<String> = sorted.into();
        let mut order = Vec::new();
        while let Some(node) = queue.pop_front() {
            order.push(node.clone());
            if let Some(children) = adj.get(&node) {
                let mut ready = Vec::new();
                for c in children {
                    if let Some(d) = indegree.get_mut(c) {
                        *d = d.saturating_sub(1);
                        if *d == 0 {
                            ready.push(c.clone());
                        }
                    }
                }
                ready.sort();
                for r in ready {
                    queue.push_back(r);
                }
            }
        }
        order
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCandidate {
    pub id: String,
    pub current: String,
    pub latest: String,
    pub kind: String,
    pub breaking: bool,
    pub release_notes: String,
}

pub fn plan_updates(
    installed: &[PluginManifest],
    available: &BTreeMap<String, String>,
) -> Vec<UpdateCandidate> {
    let mut out = Vec::new();
    for m in installed {
        let Some(latest_raw) = available.get(&m.id) else {
            continue;
        };
        let (Some(cur), Some(latest)) = (m.version_parsed(), Version::parse(latest_raw)) else {
            continue;
        };
        if latest <= cur {
            continue;
        }
        let kind = if latest.major > cur.major {
            "major"
        } else if latest.minor > cur.minor {
            "minor"
        } else {
            "patch"
        };
        out.push(UpdateCandidate {
            id: m.id.clone(),
            current: cur.to_string(),
            latest: latest.to_string(),
            kind: kind.to_string(),
            breaking: kind == "major",
            release_notes: String::new(),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdatePlan {
    pub batch: Vec<String>,
    pub safe: Vec<String>,
    pub risky: Vec<String>,
    pub backup_before: bool,
    pub rollback_on_failure: bool,
}

impl UpdatePlan {
    pub fn from_candidates(candidates: &[UpdateCandidate]) -> Self {
        let batch: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let risky: Vec<String> = candidates
            .iter()
            .filter(|c| c.breaking)
            .map(|c| c.id.clone())
            .collect();
        let safe: Vec<String> = candidates
            .iter()
            .filter(|c| !c.breaking)
            .map(|c| c.id.clone())
            .collect();
        Self {
            batch,
            safe,
            risky,
            backup_before: true,
            rollback_on_failure: true,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{} 个更新 (安全 {}, 风险 {})",
            self.batch.len(),
            self.safe.len(),
            self.risky.len()
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub id: String,
    pub file: String,
    pub revision: u64,
    pub content: String,
    pub digest: String,
    pub author: String,
    pub message: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

impl ConfigSnapshot {
    pub fn new(file: &str, content: &str, author: &str, message: &str) -> Self {
        Self {
            id: crate::crypto::random_token(12),
            file: file.to_string(),
            revision: 1,
            content: content.to_string(),
            digest: crate::crypto::sha256_hex(content.as_bytes()),
            author: author.to_string(),
            message: message.to_string(),
            at: chrono::Utc::now(),
        }
    }

    pub fn matches(&self, content: &str) -> bool {
        crate::crypto::sha256_hex(content.as_bytes()) == self.digest
    }
}

pub struct ConfigHistory {
    snapshots: Vec<ConfigSnapshot>,
}

impl ConfigHistory {
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
        }
    }

    pub fn commit(&mut self, file: &str, content: &str, author: &str, message: &str) -> ConfigSnapshot {
        let rev = self
            .snapshots
            .iter()
            .filter(|s| s.file == file)
            .map(|s| s.revision)
            .max()
            .unwrap_or(0)
            + 1;
        let mut snap = ConfigSnapshot::new(file, content, author, message);
        snap.revision = rev;
        self.snapshots.push(snap.clone());
        snap
    }

    pub fn history(&self, file: &str) -> Vec<&ConfigSnapshot> {
        let mut out: Vec<&ConfigSnapshot> = self
            .snapshots
            .iter()
            .filter(|s| s.file == file)
            .collect();
        out.sort_by_key(|s| s.revision);
        out
    }

    pub fn latest(&self, file: &str) -> Option<&ConfigSnapshot> {
        self.history(file).into_iter().next_back()
    }

    pub fn diff(&self, file: &str, from_rev: u64, to_rev: u64) -> Option<ConfigDiff> {
        let a = self
            .snapshots
            .iter()
            .find(|s| s.file == file && s.revision == from_rev)?;
        let b = self
            .snapshots
            .iter()
            .find(|s| s.file == file && s.revision == to_rev)?;
        Some(ConfigDiff::between(a, b))
    }
}

impl Default for ConfigHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigDiff {
    pub file: String,
    pub from_rev: u64,
    pub to_rev: u64,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<(String, String, String)>,
}

impl ConfigDiff {
    pub fn between(a: &ConfigSnapshot, b: &ConfigSnapshot) -> Self {
        let map_a = parse_kv(&a.content);
        let map_b = parse_kv(&b.content);
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        for (k, v) in &map_b {
            match map_a.get(k) {
                None => added.push(k.clone()),
                Some(old) if old != v => changed.push((k.clone(), old.clone(), v.clone())),
                _ => {}
            }
        }
        for k in map_a.keys() {
            if !map_b.contains_key(k) {
                removed.push(k.clone());
            }
        }
        Self {
            file: a.file.clone(),
            from_rev: a.revision,
            to_rev: b.revision,
            added,
            removed,
            changed,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn describe(&self) -> String {
        format!(
            "+{} -{} ~{}",
            self.added.len(),
            self.removed.len(),
            self.changed.len()
        )
    }
}

fn parse_kv(content: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        } else if let Some((k, v)) = trimmed.split_once(':') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriftReport {
    pub file: String,
    pub drifted: bool,
    pub keys: Vec<String>,
    pub detail: String,
}

pub fn detect_drift(expected: &str, actual: &str, file: &str) -> DriftReport {
    let e = parse_kv(expected);
    let a = parse_kv(actual);
    let mut keys = Vec::new();
    for (k, v) in &e {
        match a.get(k) {
            None => keys.push(format!("{k} 缺失")),
            Some(av) if av != v => keys.push(format!("{k}: 期望 {v} 实际 {av}")),
            _ => {}
        }
    }
    for k in a.keys() {
        if !e.contains_key(k) {
            keys.push(format!("{k} 多余"));
        }
    }
    let drifted = !keys.is_empty();
    DriftReport {
        file: file.to_string(),
        drifted,
        detail: if drifted {
            format!("{} 项不一致", keys.len())
        } else {
            "一致".to_string()
        },
        keys,
    }
}

pub fn batch_apply(targets: &[String], expected: &str) -> Vec<(String, String)> {
    targets
        .iter()
        .map(|t| (t.clone(), expected.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_and_display() {
        let v = Version::parse("1.20.4").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 20);
        assert_eq!(v.patch, 4);
        assert_eq!(v.to_string(), "1.20.4");
        assert_eq!(Version::parse("2.0").unwrap().patch, 0);
        assert!(Version::parse("").is_none());
        assert_eq!(Version::parse("1.2.3-beta").unwrap().patch, 3);
    }

    #[test]
    fn version_constraints() {
        let v = Version::parse("1.20.4").unwrap();
        assert!(v.satisfies(">=1.20.0"));
        assert!(!v.satisfies(">=1.21.0"));
        assert!(v.satisfies(">=1.20.0,<2.0.0"));
        assert!(v.satisfies("^1.20"));
        assert!(!v.satisfies("^2.0"));
        assert!(v.satisfies("~1.20"));
        assert!(!v.satisfies("~1.19"));
        assert!(v.satisfies("1.20.4"));
        assert!(!v.satisfies("1.20.3"));
        assert!(v.satisfies("*"));
    }

    #[test]
    fn version_bumping() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.bump("major").to_string(), "2.0.0");
        assert_eq!(v.bump("minor").to_string(), "1.3.0");
        assert_eq!(v.bump("patch").to_string(), "1.2.4");
    }

    #[test]
    fn resolver_orders_dependencies() {
        let manifests = vec![
            PluginManifest::new("app", "1.0.0").with_dep(Dependency::required("lib", ">=1.0.0")),
            PluginManifest::new("lib", "1.2.0"),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(res.ok);
        let lib_pos = res.install_order.iter().position(|s| s == "lib").unwrap();
        let app_pos = res.install_order.iter().position(|s| s == "app").unwrap();
        assert!(lib_pos < app_pos);
    }

    #[test]
    fn resolver_reports_missing_dependency() {
        let manifests = vec![
            PluginManifest::new("app", "1.0.0").with_dep(Dependency::required("ghost", ">=2.0.0")),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(!res.ok);
        assert_eq!(res.missing, vec!["ghost".to_string()]);
        assert!(res.errors().iter().any(|i| i.kind == IssueKind::MissingDependency));
    }

    #[test]
    fn resolver_optional_dependency_not_required() {
        let manifests = vec![
            PluginManifest::new("app", "1.0.0").with_dep(Dependency::optional("maybe", ">=1.0.0")),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(res.ok);
        assert!(res.missing.is_empty());
    }

    #[test]
    fn resolver_detects_version_conflict() {
        let manifests = vec![
            PluginManifest::new("app", "1.0.0").with_dep(Dependency::required("lib", ">=2.0.0")),
            PluginManifest::new("lib", "1.5.0"),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(!res.ok);
        assert!(res.errors().iter().any(|i| i.kind == IssueKind::VersionConflict));
    }

    #[test]
    fn resolver_detects_duplicates() {
        let manifests = vec![
            PluginManifest::new("dup", "1.0.0"),
            PluginManifest::new("dup", "1.1.0"),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(!res.ok);
        assert!(res.errors().iter().any(|i| i.kind == IssueKind::DuplicatePlugin));
    }

    #[test]
    fn resolver_detects_loader_and_mc_mismatch() {
        let manifests = vec![
            PluginManifest::new("fabricmod", "1.0.0")
                .with_loader("fabric")
                .with_mc(">=1.21.0"),
        ];
        let res = Resolver::new(&manifests)
            .for_target("1.20.4", "paper", 21)
            .resolve();
        assert!(!res.ok);
        let kinds: Vec<IssueKind> = res.issues.iter().map(|i| i.kind.clone()).collect();
        assert!(kinds.contains(&IssueKind::LoaderMismatch));
        assert!(kinds.contains(&IssueKind::McVersionMismatch));
    }

    #[test]
    fn resolver_detects_java_too_old() {
        let mut m = PluginManifest::new("modern", "1.0.0");
        m.java_major = Some(21);
        let res = Resolver::new(&[m]).for_target("1.20.4", "paper", 17).resolve();
        assert!(res.issues.iter().any(|i| i.kind == IssueKind::JavaTooOld));
        let ok = Resolver::new(&[PluginManifest {
            java_major: Some(21),
            ..PluginManifest::new("modern", "1.0.0")
        }])
        .for_target("1.20.4", "paper", 21)
        .resolve();
        assert!(!ok.issues.iter().any(|i| i.kind == IssueKind::JavaTooOld));
    }

    #[test]
    fn resolver_detects_explicit_conflict() {
        let manifests = vec![
            PluginManifest::new("a", "1.0.0"),
            PluginManifest::new("b", "1.0.0").with_dep(Dependency::conflicts("a", "*")),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(!res.ok);
        assert!(res.errors().iter().any(|i| i.kind == IssueKind::ExplicitConflict));
    }

    #[test]
    fn resolver_detects_cycle() {
        let manifests = vec![
            PluginManifest::new("a", "1.0.0").with_dep(Dependency::required("b", "*")),
            PluginManifest::new("b", "1.0.0").with_dep(Dependency::required("a", "*")),
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(!res.ok);
        assert!(res.errors().iter().any(|i| i.kind == IssueKind::DependencyCycle));
    }

    #[test]
    fn resolver_handles_provides() {
        let manifests = vec![
            PluginManifest::new("impl", "2.0.0").with_dep(Dependency::required("api", ">=1.0.0")),
            {
                let mut provider = PluginManifest::new("provider", "1.0.0");
                provider.provides.push("api".into());
                provider
            },
        ];
        let res = Resolver::new(&manifests).resolve();
        assert!(res.ok, "{:?}", res.issues);
        assert!(res.install_order.contains(&"impl".to_string()));
    }

    #[test]
    fn resolver_flags_client_only() {
        let mut m = PluginManifest::new("shader", "1.0.0");
        m.side = "client".into();
        let res = Resolver::new(&[m.clone()]).resolve();
        assert!(res.warnings().iter().any(|i| i.kind == IssueKind::SideMismatch));
        let allowed = Resolver::new(&[m]).allow_client_only(true).resolve();
        assert!(allowed.ok);
    }

    #[test]
    fn update_planning() {
        let installed = vec![
            PluginManifest::new("a", "1.0.0"),
            PluginManifest::new("b", "2.0.0"),
            PluginManifest::new("c", "1.5.0"),
        ];
        let mut available = BTreeMap::new();
        available.insert("a".to_string(), "1.0.1".to_string());
        available.insert("b".to_string(), "3.0.0".to_string());
        available.insert("c".to_string(), "1.5.0".to_string());
        let cands = plan_updates(&installed, &available);
        assert_eq!(cands.len(), 2);
        assert!(cands.iter().any(|c| c.id == "b" && c.breaking));
        assert!(cands.iter().any(|c| c.id == "a" && !c.breaking));
        let plan = UpdatePlan::from_candidates(&cands);
        assert!(plan.backup_before);
        assert!(plan.rollback_on_failure);
        assert_eq!(plan.risky, vec!["b".to_string()]);
        assert!(plan.describe().contains("2 个更新"));
    }

    #[test]
    fn config_history_and_rollback() {
        let mut h = ConfigHistory::new();
        h.commit("server.properties", "max-players=20\nmotd=Hi", "root", "初始");
        h.commit("server.properties", "max-players=50\nmotd=Hi", "root", "扩容");
        let hist = h.history("server.properties");
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[1].revision, 2);
        let diff = h.diff("server.properties", 1, 2).unwrap();
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].0, "max-players");
        assert!(!diff.is_empty());
        let latest = h.latest("server.properties").unwrap();
        assert!(latest.matches("max-players=50\nmotd=Hi"));
    }

    #[test]
    fn config_diff_detects_add_remove() {
        let a = ConfigSnapshot::new("c.yml", "x=1\ny=2", "root", "");
        let b = ConfigSnapshot::new("c.yml", "y=2\nz=3", "root", "");
        let d = ConfigDiff::between(&a, &b);
        assert_eq!(d.added, vec!["z"]);
        assert_eq!(d.removed, vec!["x"]);
        assert!(d.changed.is_empty());
        assert_eq!(d.describe(), "+1 -1 ~0");
    }

    #[test]
    fn drift_detection() {
        let expected = "max-players=20\ndifficulty=hard";
        let same = detect_drift(expected, expected, "server.properties");
        assert!(!same.drifted);
        assert_eq!(same.detail, "一致");
        let changed = detect_drift(expected, "max-players=99\ndifficulty=hard\npvp=true", "server.properties");
        assert!(changed.drifted);
        assert!(changed.keys.iter().any(|k| k.contains("max-players")));
        assert!(changed.keys.iter().any(|k| k.contains("pv p") || k.contains("pv")));
    }

    #[test]
    fn batch_apply_maps_targets() {
        let targets = vec!["s1".to_string(), "s2".to_string()];
        let applied = batch_apply(&targets, "difficulty=hard");
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].0, "s1");
        assert_eq!(applied[0].1, "difficulty=hard");
    }
}
