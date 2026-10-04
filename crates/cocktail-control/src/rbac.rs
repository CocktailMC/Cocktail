use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::crypto;

pub const ALL_PERMISSIONS: &[&str] = &[
    "view",
    "start",
    "stop",
    "console",
    "logs",
    "files",
    "files.read",
    "files.write",
    "plugins",
    "plugins.install",
    "players",
    "players.detail",
    "players.kick",
    "players.ban",
    "players.pardon",
    "players.op",
    "players.deop",
    "players.whitelist",
    "players.gamemode",
    "players.teleport",
    "players.give",
    "players.effect",
    "players.kill",
    "players.clear",
    "rcon",
    "rcon.setup",
    "backups",
    "settings",
    "automations",
    "netops.view",
    "netops.write",
    "nodes",
    "nodes.manage",
    "users",
    "roles.manage",
    "apikeys.manage",
    "2fa.manage",
    "audit.read",
    "metrics.read",
];

pub fn known_permission(perm: &str) -> bool {
    ALL_PERMISSIONS.contains(&perm)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: BTreeSet<String>,
    pub builtin: bool,
    pub inherits: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Role {
    pub fn new(id: &str, name: &str, perms: &[&str]) -> Self {
        let now = Utc::now();
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            permissions: perms.iter().map(|p| p.to_string()).collect(),
            builtin: false,
            inherits: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn builtin(mut self) -> Self {
        self.builtin = true;
        self
    }

    pub fn with_desc(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn grants(&self, perm: &str) -> bool {
        self.permissions.contains(perm)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scope {
    pub kind: String,
    pub id: String,
}

impl Scope {
    pub fn global() -> Self {
        Self {
            kind: "global".into(),
            id: "*".into(),
        }
    }

    pub fn instance(id: &str) -> Self {
        Self {
            kind: "instance".into(),
            id: id.to_string(),
        }
    }

    pub fn node(id: &str) -> Self {
        Self {
            kind: "node".into(),
            id: id.to_string(),
        }
    }

    pub fn matches(&self, other: &Scope) -> bool {
        if self.kind == "global" && self.id == "*" {
            return true;
        }
        self.kind == other.kind && (self.id == "*" || self.id == other.id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub subject: String,
    pub subject_kind: String,
    pub role_id: String,
    pub scope: Scope,
    pub granted_by: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub reason: String,
}

impl Grant {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.map(|t| t <= now).unwrap_or(false)
    }

    pub fn remaining_secs(&self, now: DateTime<Utc>) -> Option<i64> {
        self.expires_at.map(|t| (t - now).num_seconds())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserGroup {
    pub id: String,
    pub name: String,
    pub members: BTreeSet<String>,
    pub roles: BTreeSet<String>,
    pub created_at: DateTime<Utc>,
}

pub struct PolicyEngine {
    roles: BTreeMap<String, Role>,
    grants: Vec<Grant>,
    groups: BTreeMap<String, UserGroup>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self {
            roles: BTreeMap::new(),
            grants: Vec::new(),
            groups: BTreeMap::new(),
        }
    }

    pub fn with_builtins() -> Self {
        let mut e = Self::new();
        e.add_role(
            Role::new("observer", "观察员", &["view", "metrics.read"])
                .builtin()
                .with_desc("只读查看"),
        );
        e.add_role(
            Role::new(
                "support",
                "客服",
                &[
                    "view",
                    "start",
                    "stop",
                    "console",
                    "logs",
                    "players",
                    "players.detail",
                    "players.kick",
                    "players.ban",
                    "players.pardon",
                    "players.whitelist",
                    "rcon",
                    "backups",
                    "netops.view",
                ],
            )
            .builtin()
            .with_desc("处理玩家问题"),
        );
        e.add_role(
            Role::new(
                "developer",
                "开发者",
                &[
                    "view",
                    "console",
                    "logs",
                    "files",
                    "files.read",
                    "files.write",
                    "plugins",
                    "plugins.install",
                    "start",
                    "stop",
                    "rcon",
                ],
            )
            .builtin()
            .with_desc("改插件与文件"),
        );
        e.add_role(
            Role::new(
                "admin",
                "管理员",
                &[
                    "view",
                    "start",
                    "stop",
                    "console",
                    "logs",
                    "files",
                    "files.read",
                    "files.write",
                    "plugins",
                    "plugins.install",
                    "players",
                    "players.detail",
                    "players.kick",
                    "players.ban",
                    "players.pardon",
                    "players.op",
                    "players.deop",
                    "players.whitelist",
                    "players.gamemode",
                    "players.teleport",
                    "players.give",
                    "players.effect",
                    "players.kill",
                    "players.clear",
                    "rcon",
                    "rcon.setup",
                    "backups",
                    "settings",
                    "automations",
                    "netops.view",
                    "netops.write",
                    "nodes",
                    "audit.read",
                    "metrics.read",
                ],
            )
            .builtin()
            .with_desc("日常运营"),
        );
        e.add_role(
            Role::new("superadmin", "Owner", ALL_PERMISSIONS).builtin().with_desc("全部权限"),
        );
        e
    }

    pub fn add_role(&mut self, role: Role) {
        self.roles.insert(role.id.clone(), role);
    }

    pub fn remove_role(&mut self, id: &str) -> bool {
        if self.roles.get(id).map(|r| r.builtin).unwrap_or(false) {
            return false;
        }
        self.roles.remove(id).is_some()
    }

    pub fn role(&self, id: &str) -> Option<&Role> {
        self.roles.get(id)
    }

    pub fn roles(&self) -> Vec<Role> {
        self.roles.values().cloned().collect()
    }

    pub fn effective_permissions(&self, role_id: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut cursor = Some(role_id.to_string());
        let mut hops = 0;
        while let Some(id) = cursor {
            if hops > 16 {
                break;
            }
            let Some(role) = self.roles.get(&id) else {
                break;
            };
            for p in &role.permissions {
                out.insert(p.clone());
            }
            cursor = role.inherits.clone();
            hops += 1;
        }
        out
    }

    pub fn add_group(&mut self, group: UserGroup) {
        self.groups.insert(group.id.clone(), group);
    }

    pub fn group(&self, id: &str) -> Option<&UserGroup> {
        self.groups.get(id)
    }

    pub fn groups(&self) -> Vec<UserGroup> {
        self.groups.values().cloned().collect()
    }

    pub fn groups_for_user(&self, user: &str) -> Vec<&UserGroup> {
        self.groups
            .values()
            .filter(|g| g.members.contains(user))
            .collect()
    }

    pub fn add_grant(&mut self, grant: Grant) {
        self.grants.push(grant);
    }

    pub fn revoke_grant(&mut self, id: &str) -> bool {
        let before = self.grants.len();
        self.grants.retain(|g| g.id != id);
        self.grants.len() != before
    }

    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    pub fn purge_expired(&mut self, now: DateTime<Utc>) -> usize {
        let before = self.grants.len();
        self.grants.retain(|g| !g.is_expired(now));
        before - self.grants.len()
    }

    pub fn effective_for(
        &self,
        user: &str,
        base_role: &str,
        scope: &Scope,
    ) -> BTreeSet<String> {
        let now = Utc::now();
        let mut perms = self.effective_permissions(base_role);
        for group in self.groups_for_user(user) {
            for role_id in &group.roles {
                for p in self.effective_permissions(role_id) {
                    perms.insert(p);
                }
            }
        }
        for grant in &self.grants {
            if grant.subject_kind != "user" || grant.subject != user {
                continue;
            }
            if grant.is_expired(now) {
                continue;
            }
            if !grant.scope.matches(scope) {
                continue;
            }
            for p in self.effective_permissions(&grant.role_id) {
                perms.insert(p);
            }
        }
        perms
    }

    pub fn can(&self, user: &str, base_role: &str, scope: &Scope, perm: &str) -> bool {
        if perm.is_empty() || perm == "view" {
            return true;
        }
        let perms = self.effective_for(user, base_role, scope);
        if perms.contains(perm) {
            return true;
        }
        if let Some((head, tail)) = perm.split_once('.') {
            if perms.contains(&format!("{head}.*")) {
                return true;
            }
            if tail.is_empty() {
                return false;
            }
        }
        false
    }

    pub fn temporary_grants(&self, now: DateTime<Utc>) -> Vec<&Grant> {
        self.grants
            .iter()
            .filter(|g| !g.is_expired(now) && g.expires_at.is_some())
            .collect()
    }

    pub fn describe(&self) -> Vec<(String, usize)> {
        self.roles
            .values()
            .map(|r| (r.id.clone(), self.effective_permissions(&r.id).len()))
            .collect()
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub hash: String,
    pub scopes: BTreeSet<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: u64,
    pub revoked: bool,
}

impl ApiKey {
    pub fn generate(name: &str, created_by: &str, scopes: &[&str]) -> (Self, String) {
        let secret = format!("ck_{}", crypto::random_token(40));
        let prefix = secret[..11].to_string();
        let hash = crypto::sha256_hex(secret.as_bytes());
        let key = Self {
            id: crypto::random_token(16),
            name: name.to_string(),
            prefix,
            hash,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            created_by: created_by.to_string(),
            created_at: Utc::now(),
            expires_at: None,
            last_used_at: None,
            use_count: 0,
            revoked: false,
        };
        (key, secret)
    }

    pub fn verify(&self, presented: &str) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(exp) = self.expires_at {
            if exp <= Utc::now() {
                return false;
            }
        }
        let hash = crypto::sha256_hex(presented.as_bytes());
        crypto::ct_eq(hash.as_bytes(), self.hash.as_bytes())
    }

    pub fn touch(&mut self) {
        self.last_used_at = Some(Utc::now());
        self.use_count = self.use_count.saturating_add(1);
    }

    pub fn allows(&self, perm: &str) -> bool {
        if self.scopes.contains("*") {
            return true;
        }
        if self.scopes.contains(perm) {
            return true;
        }
        if let Some((head, _)) = perm.split_once('.') {
            if self.scopes.contains(head) {
                return true;
            }
        }
        false
    }

    pub fn mask(&self) -> String {
        format!("{}...", self.prefix)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSession {
    pub id: String,
    pub user: String,
    pub ip: String,
    pub user_agent: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
}

impl DeviceSession {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        !self.revoked && self.expires_at > now
    }

    pub fn is_new_device(&self, other: &DeviceSession) -> bool {
        self.user_agent != other.user_agent
    }

    pub fn label(&self) -> String {
        let ua = &self.user_agent;
        let browser = if ua.contains("Firefox") {
            "Firefox"
        } else if ua.contains("Edg") {
            "Edge"
        } else if ua.contains("Chrome") {
            "Chrome"
        } else if ua.contains("Safari") {
            "Safari"
        } else if ua.contains("curl") {
            "curl"
        } else {
            "未知客户端"
        };
        let os = if ua.contains("Windows") {
            "Windows"
        } else if ua.contains("Android") {
            "Android"
        } else if ua.contains("iPhone") || ua.contains("iPad") {
            "iOS"
        } else if ua.contains("Mac OS") {
            "macOS"
        } else if ua.contains("Linux") {
            "Linux"
        } else {
            "未知系统"
        };
        format!("{browser} / {os} @ {}", self.ip)
    }
}

pub struct SessionRegistry {
    sessions: BTreeMap<String, DeviceSession>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, session: DeviceSession) {
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn get(&self, id: &str) -> Option<&DeviceSession> {
        self.sessions.get(id)
    }

    pub fn for_user(&self, user: &str) -> Vec<&DeviceSession> {
        self.sessions
            .values()
            .filter(|s| s.user == user)
            .collect()
    }

    pub fn active_for_user(&self, user: &str, now: DateTime<Utc>) -> Vec<&DeviceSession> {
        self.sessions
            .values()
            .filter(|s| s.user == user && s.is_active(now))
            .collect()
    }

    pub fn revoke(&mut self, id: &str) -> bool {
        match self.sessions.get_mut(id) {
            Some(s) => {
                s.revoked = true;
                true
            }
            None => false,
        }
    }

    pub fn revoke_all(&mut self, user: &str) -> usize {
        let mut n = 0;
        for s in self.sessions.values_mut() {
            if s.user == user && !s.revoked {
                s.revoked = true;
                n += 1;
            }
        }
        n
    }

    pub fn prune_expired(&mut self, now: DateTime<Utc>) -> usize {
        let before = self.sessions.len();
        self.sessions.retain(|_, s| s.expires_at > now);
        before - self.sessions.len()
    }

    pub fn is_anomalous(&self, user: &str, ip: &str, ua: &str) -> bool {
        let known_ip = self
            .sessions
            .values()
            .any(|s| s.user == user && s.ip == ip);
        let known_ua = self
            .sessions
            .values()
            .any(|s| s.user == user && s.user_agent == ua);
        !known_ip || !known_ua
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PasswordPolicy {
    pub min_len: usize,
    pub require_upper: bool,
    pub require_lower: bool,
    pub require_digit: bool,
    pub require_symbol: bool,
    pub history_depth: usize,
    pub max_age_days: u64,
    pub deny_username: bool,
    pub deny_common: bool,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_len: 10,
            require_upper: true,
            require_lower: true,
            require_digit: true,
            require_symbol: false,
            history_depth: 5,
            max_age_days: 0,
            deny_username: true,
            deny_common: true,
        }
    }
}

const COMMON_WEAK: &[&str] = &[
    "password",
    "123456",
    "12345678",
    "123456789",
    "qwerty",
    "abc123",
    "111111",
    "letmein",
    "admin",
    "welcome",
    "monkey",
    "dragon",
    "master",
    "iloveyou",
    "sunshine",
    "princess",
    "football",
    "changeme",
    "root",
    "passw0rd",
    "admin123",
    "qwerty123",
    "1q2w3e4r",
    "a1b2c3d4",
    "cocktail",
];

impl PasswordPolicy {
    pub fn check(&self, username: &str, password: &str) -> Vec<String> {
        let mut issues = Vec::new();
        if password.chars().count() < self.min_len {
            issues.push(format!("长度至少 {} 位", self.min_len));
        }
        if self.require_upper && !password.chars().any(|c| c.is_ascii_uppercase()) {
            issues.push("需要至少一个大写字母".into());
        }
        if self.require_lower && !password.chars().any(|c| c.is_ascii_lowercase()) {
            issues.push("需要至少一个小写字母".into());
        }
        if self.require_digit && !password.chars().any(|c| c.is_ascii_digit()) {
            issues.push("需要至少一个数字".into());
        }
        if self.require_symbol
            && !password
                .chars()
                .any(|c| !c.is_ascii_alphanumeric())
        {
            issues.push("需要至少一个符号".into());
        }
        if self.deny_username && !username.is_empty() {
            let lower = password.to_ascii_lowercase();
            if lower.contains(&username.to_ascii_lowercase()) {
                issues.push("不能包含用户名".into());
            }
        }
        if self.deny_common {
            let lower = password.to_ascii_lowercase();
            if COMMON_WEAK.iter().any(|w| lower.contains(w)) {
                issues.push("不能使用常见弱密码".into());
            }
            if is_keyboard_run(&lower) {
                issues.push("不能使用键盘连续字符".into());
            }
            if is_single_repeat(password) {
                issues.push("不能使用重复字符".into());
            }
        }
        issues
    }

    pub fn is_acceptable(&self, username: &str, password: &str) -> bool {
        self.check(username, password).is_empty()
    }

    pub fn strength(&self, password: &str) -> u8 {
        let mut score = 0u8;
        let len = password.chars().count();
        if len >= 8 {
            score += 1;
        }
        if len >= 12 {
            score += 1;
        }
        if len >= 16 {
            score += 1;
        }
        if password.chars().any(|c| c.is_ascii_lowercase()) {
            score += 1;
        }
        if password.chars().any(|c| c.is_ascii_uppercase()) {
            score += 1;
        }
        if password.chars().any(|c| c.is_ascii_digit()) {
            score += 1;
        }
        if password.chars().any(|c| !c.is_ascii_alphanumeric()) {
            score += 1;
        }
        if COMMON_WEAK
            .iter()
            .any(|w| password.to_ascii_lowercase().contains(w))
        {
            score = score.saturating_sub(3);
        }
        score.min(5)
    }

    pub fn label(score: u8) -> &'static str {
        match score {
            0 | 1 => "很弱",
            2 => "弱",
            3 => "中等",
            4 => "强",
            _ => "很强",
        }
    }

    pub fn is_reused(&self, password: &str, history: &[String]) -> bool {
        history.iter().any(|h| h == password)
    }

    pub fn is_expired(&self, changed_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        if self.max_age_days == 0 {
            return false;
        }
        (now - changed_at).num_days() as u64 >= self.max_age_days
    }
}

fn is_keyboard_run(lower: &str) -> bool {
    const RUNS: &[&str] = &[
        "qwerty",
        "asdf",
        "zxcv",
        "1234",
        "2345",
        "3456",
        "4567",
        "5678",
        "6789",
        "7890",
        "abcd",
        "bcde",
        "cdef",
    ];
    RUNS.iter().any(|r| lower.contains(r))
}

fn is_single_repeat(password: &str) -> bool {
    let mut chars = password.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    password.chars().count() >= 4 && chars.all(|c| c == first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_roles_have_expected_shape() {
        let e = PolicyEngine::with_builtins();
        assert!(e.role("superadmin").unwrap().permissions.len() >= 30);
        assert!(e.role("observer").unwrap().grants("view"));
        assert!(!e.role("observer").unwrap().grants("players.kick"));
        assert!(e.role("support").unwrap().grants("players.kick"));
        assert!(!e.role("support").unwrap().grants("rcon.setup"));
        assert!(e.role("admin").unwrap().grants("rcon.setup"));
    }

    #[test]
    fn role_inheritance_chains() {
        let mut e = PolicyEngine::with_builtins();
        let mut child = Role::new("auditor", "审计员", &["audit.read"]);
        child.inherits = Some("observer".into());
        e.add_role(child);
        let perms = e.effective_permissions("auditor");
        assert!(perms.contains("audit.read"));
        assert!(perms.contains("view"));
        assert!(perms.contains("metrics.read"));
    }

    #[test]
    fn inheritance_cycle_is_bounded() {
        let mut e = PolicyEngine::new();
        let mut a = Role::new("a", "A", &["view"]);
        a.inherits = Some("b".into());
        let mut b = Role::new("b", "B", &["logs"]);
        b.inherits = Some("a".into());
        e.add_role(a);
        e.add_role(b);
        let perms = e.effective_permissions("a");
        assert!(perms.contains("view"));
        assert!(perms.contains("logs"));
    }

    #[test]
    fn builtin_roles_cannot_be_removed() {
        let mut e = PolicyEngine::with_builtins();
        assert!(!e.remove_role("admin"));
        e.add_role(Role::new("temp", "临时", &["view"]));
        assert!(e.remove_role("temp"));
    }

    #[test]
    fn scope_matching() {
        let global = Scope::global();
        let inst_a = Scope::instance("a");
        let inst_b = Scope::instance("b");
        assert!(global.matches(&inst_a));
        assert!(inst_a.matches(&inst_a));
        assert!(!inst_a.matches(&inst_b));
        let node = Scope::node("n1");
        assert!(!inst_a.matches(&node));
    }

    #[test]
    fn per_instance_grant_isolated() {
        let mut e = PolicyEngine::with_builtins();
        e.add_grant(Grant {
            id: "g1".into(),
            subject: "alice".into(),
            subject_kind: "user".into(),
            role_id: "admin".into(),
            scope: Scope::instance("survival"),
            granted_by: "root".into(),
            granted_at: Utc::now(),
            expires_at: None,
            reason: "负责生存服".into(),
        });
        let survival = Scope::instance("survival");
        let creative = Scope::instance("creative");
        assert!(e.can("alice", "observer", &survival, "players.ban"));
        assert!(!e.can("alice", "observer", &creative, "players.ban"));
        assert!(!e.can("bob", "observer", &survival, "players.ban"));
    }

    #[test]
    fn expired_grant_is_ignored() {
        let mut e = PolicyEngine::with_builtins();
        e.add_grant(Grant {
            id: "g2".into(),
            subject: "carol".into(),
            subject_kind: "user".into(),
            role_id: "admin".into(),
            scope: Scope::global(),
            granted_by: "root".into(),
            granted_at: Utc::now() - chrono::Duration::hours(48),
            expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
            reason: "临时".into(),
        });
        assert!(!e.can("carol", "observer", &Scope::global(), "players.ban"));
        assert_eq!(e.purge_expired(Utc::now()), 1);
    }

    #[test]
    fn temporary_grant_listing() {
        let mut e = PolicyEngine::with_builtins();
        let now = Utc::now();
        e.add_grant(Grant {
            id: "temp".into(),
            subject: "dave".into(),
            subject_kind: "user".into(),
            role_id: "support".into(),
            scope: Scope::global(),
            granted_by: "root".into(),
            granted_at: now,
            expires_at: Some(now + chrono::Duration::hours(24)),
            reason: "值守".into(),
        });
        assert_eq!(e.temporary_grants(now).len(), 1);
        assert_eq!(e.temporary_grants(now)[0].id, "temp");
    }

    #[test]
    fn group_grants_to_members() {
        let mut e = PolicyEngine::with_builtins();
        let mut g = UserGroup {
            id: "ops".into(),
            name: "运维组".into(),
            members: BTreeSet::new(),
            roles: BTreeSet::new(),
            created_at: Utc::now(),
        };
        g.members.insert("erin".into());
        g.roles.insert("admin".into());
        e.add_group(g);
        assert_eq!(e.groups_for_user("erin").len(), 1);
        assert!(e.can("erin", "observer", &Scope::global(), "rcon.setup"));
        assert!(!e.can("frank", "observer", &Scope::global(), "rcon.setup"));
    }

    #[test]
    fn permission_prefix_expansion() {
        let e = PolicyEngine::with_builtins();
        assert!(e.can("x", "support", &Scope::global(), "players.kick"));
        assert!(!e.can("x", "support", &Scope::global(), "players.op"));
        assert!(e.can("x", "observer", &Scope::global(), "view"));
    }

    #[test]
    fn api_key_lifecycle() {
        let (mut key, secret) = ApiKey::generate("ci", "root", &["backups", "view"]);
        assert!(secret.starts_with("ck_"));
        assert!(key.verify(&secret));
        assert!(!key.verify("ck_wrong"));
        assert!(key.allows("backups"));
        assert!(key.allows("backups.read"));
        assert!(!key.allows("users"));
        key.touch();
        assert_eq!(key.use_count, 1);
        assert!(key.last_used_at.is_some());
        key.revoked = true;
        assert!(!key.verify(&secret));
        assert!(key.mask().ends_with("..."));
    }

    #[test]
    fn api_key_wildcard_scope() {
        let (key, secret) = ApiKey::generate("full", "root", &["*"]);
        assert!(key.allows("anything"));
        assert!(key.verify(&secret));
    }

    #[test]
    fn api_key_expiry() {
        let (mut key, secret) = ApiKey::generate("old", "root", &["view"]);
        key.expires_at = Some(Utc::now() - chrono::Duration::minutes(1));
        assert!(!key.verify(&secret));
    }

    #[test]
    fn session_registry_lifecycle() {
        let mut reg = SessionRegistry::new();
        let now = Utc::now();
        let mk = |id: &str, user: &str, ip: &str, ua: &str, exp_off: i64| DeviceSession {
            id: id.into(),
            user: user.into(),
            ip: ip.into(),
            user_agent: ua.into(),
            created_at: now,
            last_seen: now,
            expires_at: now + chrono::Duration::hours(exp_off),
            revoked: false,
        };
        reg.insert(mk("s1", "alice", "1.1.1.1", "Mozilla/5.0 Chrome/120 Windows", 1));
        reg.insert(mk("s2", "alice", "2.2.2.2", "curl/8.0 Linux", 1));
        reg.insert(mk("s3", "alice", "1.1.1.1", "old", -1));
        assert_eq!(reg.for_user("alice").len(), 3);
        assert_eq!(reg.active_for_user("alice", now).len(), 2);
        assert_eq!(reg.prune_expired(now), 1);
        assert!(reg.revoke("s1"));
        assert!(!reg.revoke("nope"));
        assert_eq!(reg.revoke_all("alice"), 1);
    }

    #[test]
    fn session_labels_and_anomaly() {
        let mut reg = SessionRegistry::new();
        let now = Utc::now();
        reg.insert(DeviceSession {
            id: "s1".into(),
            user: "alice".into(),
            ip: "1.1.1.1".into(),
            user_agent: "Mozilla/5.0 Chrome/120 Linux".into(),
            created_at: now,
            last_seen: now,
            expires_at: now + chrono::Duration::hours(1),
            revoked: false,
        });
        let s = reg.get("s1").unwrap();
        assert!(s.label().contains("Chrome"));
        assert!(s.label().contains("Linux"));
        assert!(!reg.is_anomalous("alice", "1.1.1.1", "Mozilla/5.0 Chrome/120 Linux"));
        assert!(reg.is_anomalous("alice", "9.9.9.9", "Mozilla/5.0 Chrome/120 Linux"));
        assert!(reg.is_anomalous("alice", "1.1.1.1", "Firefox/121"));
    }

    #[test]
    fn password_policy_rejects_weak() {
        let p = PasswordPolicy::default();
        assert!(!p.is_acceptable("root", "password"));
        assert!(!p.is_acceptable("root", "short1A"));
        assert!(!p.is_acceptable("root", "rootroot1A"));
        assert!(!p.is_acceptable("alice", "AliceSecure1"));
        assert!(p.is_acceptable("alice", "Zx9-Quiet-Harbor"));
        let issues = p.check("bob", "abc");
        assert!(issues.len() >= 3);
    }

    #[test]
    fn password_strength_scoring() {
        let p = PasswordPolicy::default();
        assert!(p.strength("abc") <= 2);
        assert!(p.strength("VeryLongAndMixed123!") >= 5);
        assert_eq!(PasswordPolicy::label(0), "很弱");
        assert_eq!(PasswordPolicy::label(5), "很强");
        assert!(p.strength("password123456789") < 3);
    }

    #[test]
    fn password_history_and_age() {
        let p = PasswordPolicy::default();
        let history = vec!["OldPass1".to_string(), "Older2".to_string()];
        assert!(p.is_reused("OldPass1", &history));
        assert!(!p.is_reused("Brand3New", &history));
        let now = Utc::now();
        let aged = PasswordPolicy {
            max_age_days: 90,
            ..Default::default()
        };
        assert!(aged.is_expired(now - chrono::Duration::days(91), now));
        assert!(!aged.is_expired(now - chrono::Duration::days(10), now));
        assert!(!p.is_expired(now - chrono::Duration::days(9999), now));
    }

    #[test]
    fn permission_catalog_is_consistent() {
        assert!(known_permission("players.ban"));
        assert!(known_permission("rcon.setup"));
        assert!(!known_permission("nonsense.perm"));
        let e = PolicyEngine::with_builtins();
        for role in e.roles() {
            for perm in &role.permissions {
                assert!(known_permission(perm), "未知权限位: {perm}");
            }
        }
    }
}
