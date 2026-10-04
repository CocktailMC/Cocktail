use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn parse(raw: &str) -> Option<Self> {
        let t = raw.trim().to_ascii_lowercase();
        if t.starts_with("zh") || t.starts_with("cn") {
            Some(Lang::Zh)
        } else if t.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            Lang::En => "en-US",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Lang::Zh => "简体中文",
            Lang::En => "English",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bundle {
    entries: BTreeMap<&'static str, (&'static str, &'static str)>,
}

impl Bundle {
    pub fn new() -> Self {
        let mut entries = BTreeMap::new();
        for (key, zh, en) in CATALOG {
            entries.insert(*key, (*zh, *en));
        }
        Self { entries }
    }

    pub fn keys(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }

    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&self, lang: Lang, key: &str) -> String {
        match self.entries.get(key) {
            Some((zh, en)) => match lang {
                Lang::Zh => zh.to_string(),
                Lang::En => en.to_string(),
            },
            None => key.to_string(),
        }
    }

    pub fn get_args(&self, lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
        let mut text = self.get(lang, key);
        for (k, v) in args {
            text = text.replace(&format!("{{{k}}}"), v);
        }
        text
    }

    pub fn coverage(&self, lang: Lang) -> usize {
        self.entries
            .values()
            .filter(|(zh, en)| match lang {
                Lang::Zh => !zh.is_empty(),
                Lang::En => !en.is_empty(),
            })
            .count()
    }

    pub fn missing_for(&self, lang: Lang) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|(_, (zh, en))| match lang {
                Lang::Zh => zh.is_empty(),
                Lang::En => en.is_empty(),
            })
            .map(|(k, _)| *k)
            .collect()
    }

    pub fn to_json(&self, lang: Lang) -> String {
        let mut map = serde_json::Map::new();
        for (key, (zh, en)) in &self.entries {
            let value = match lang {
                Lang::Zh => *zh,
                Lang::En => *en,
            };
            map.insert(
                (*key).to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| "{}".into())
    }
}

impl Default for Bundle {
    fn default() -> Self {
        Self::new()
    }
}

pub const CATALOG: &[(&str, &str, &str)] = &[
    ("app.name", "Cocktail 控制面板", "Cocktail Manager"),
    ("nav.instances", "实例", "Instances"),
    ("nav.players", "玩家", "Players"),
    ("nav.console", "控制台", "Console"),
    ("nav.files", "文件", "Files"),
    ("nav.backups", "备份", "Backups"),
    ("nav.nodes", "节点", "Nodes"),
    ("nav.users", "用户权限", "Users"),
    ("nav.audit", "审计日志", "Audit Log"),
    ("nav.settings", "设置", "Settings"),
    ("nav.metrics", "监控", "Metrics"),
    ("nav.workflows", "工作流", "Workflows"),
    ("nav.tickets", "工单", "Tickets"),
    ("action.start", "启动", "Start"),
    ("action.stop", "停止", "Stop"),
    ("action.restart", "重启", "Restart"),
    ("action.kill", "强制结束", "Force Kill"),
    ("action.backup", "备份", "Backup"),
    ("action.restore", "恢复", "Restore"),
    ("action.clone", "克隆", "Clone"),
    ("action.delete", "删除", "Delete"),
    ("action.save", "保存", "Save"),
    ("action.cancel", "取消", "Cancel"),
    ("action.confirm", "确认", "Confirm"),
    ("action.refresh", "刷新", "Refresh"),
    ("action.export", "导出", "Export"),
    ("action.import", "导入", "Import"),
    ("player.kick", "踢出", "Kick"),
    ("player.ban", "封禁", "Ban"),
    ("player.pardon", "解封", "Unban"),
    ("player.op", "给予 OP", "Grant OP"),
    ("player.deop", "撤销 OP", "Revoke OP"),
    ("player.whitelist", "加入白名单", "Add to Whitelist"),
    ("player.gamemode", "切换模式", "Change Gamemode"),
    ("player.teleport", "传送", "Teleport"),
    ("player.give", "给予物品", "Give Item"),
    ("player.effect", "药水效果", "Apply Effect"),
    ("player.kill", "击杀", "Kill"),
    ("player.clear", "清空背包", "Clear Inventory"),
    ("state.running", "运行中", "Running"),
    ("state.stopped", "已停止", "Stopped"),
    ("state.starting", "启动中", "Starting"),
    ("state.crashed", "已崩溃", "Crashed"),
    ("state.unknown", "未知", "Unknown"),
    ("auth.login", "登录", "Sign In"),
    ("auth.logout", "退出登录", "Sign Out"),
    ("auth.username", "用户名", "Username"),
    ("auth.password", "密码", "Password"),
    ("auth.totp", "动态验证码", "Authenticator Code"),
    (
        "auth.totp_required",
        "需要动态验证码",
        "Authenticator code required",
    ),
    (
        "auth.totp_invalid",
        "动态验证码不正确",
        "Invalid authenticator code",
    ),
    (
        "auth.session_expired",
        "登录已过期, 请重新登录",
        "Session expired, please sign in again",
    ),
    ("auth.forbidden", "权限不足", "Insufficient permission"),
    ("auth.recovery_codes", "恢复码", "Recovery Codes"),
    (
        "auth.recovery_hint",
        "请妥善保存, 每个只能使用一次",
        "Store safely, each code works once",
    ),
    ("backup.creating", "正在创建快照", "Creating snapshot"),
    ("backup.created", "快照已创建", "Snapshot created"),
    ("backup.failed", "备份失败", "Backup failed"),
    ("backup.verify", "校验快照", "Verify Snapshot"),
    ("backup.verify_ok", "校验通过", "Verification passed"),
    ("backup.verify_failed", "校验失败", "Verification failed"),
    ("backup.dedup", "去重率", "Dedup Ratio"),
    ("backup.remote", "远端存储", "Remote Storage"),
    ("backup.uploading", "正在上传", "Uploading"),
    ("rcon.title", "RCON 控制通道", "RCON Channel"),
    ("rcon.enable", "开启 RCON", "Enable RCON"),
    ("rcon.disable", "关闭 RCON", "Disable RCON"),
    ("rcon.exec", "执行命令", "Run Command"),
    ("rcon.disabled", "RCON 未开启", "RCON is disabled"),
    ("node.online", "在线", "Online"),
    ("node.offline", "离线", "Offline"),
    ("node.maintenance", "维护中", "Maintenance"),
    ("node.draining", "排空中", "Draining"),
    ("node.protocol", "协议版本", "Protocol Version"),
    ("node.migrate", "迁移实例", "Migrate Instance"),
    ("alert.cpu_high", "CPU 使用率过高", "CPU usage too high"),
    ("alert.mem_high", "内存使用率过高", "Memory usage too high"),
    ("alert.disk_low", "磁盘剩余空间不足", "Low disk space"),
    (
        "alert.backup_failed",
        "备份连续失败",
        "Repeated backup failures",
    ),
    ("alert.instance_crashed", "实例崩溃", "Instance crashed"),
    ("error.not_found", "资源不存在", "Resource not found"),
    ("error.bad_request", "请求参数有误", "Invalid request"),
    (
        "error.rate_limited",
        "请求过于频繁, 请稍后再试",
        "Too many requests, try again later",
    ),
    ("error.internal", "服务内部错误", "Internal server error"),
    ("error.offline", "节点离线", "Node offline"),
    (
        "error.instance_not_running",
        "实例未运行",
        "Instance is not running",
    ),
    (
        "confirm.delete_instance",
        "确定删除实例 {name} 吗, 该操作不可撤销",
        "Delete instance {name}, this cannot be undone",
    ),
    (
        "confirm.disable_2fa",
        "确定关闭双因素认证吗",
        "Disable two-factor authentication",
    ),
    (
        "empty.no_instances",
        "还没有实例, 创建一个开始吧",
        "No instances yet, create one to begin",
    ),
    ("empty.no_players", "当前没有在线玩家", "No players online"),
    ("empty.no_backups", "还没有备份", "No backups yet"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_parsing() {
        assert_eq!(Lang::parse("zh-CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh"), Some(Lang::Zh));
        assert_eq!(Lang::parse("en-US"), Some(Lang::En));
        assert_eq!(Lang::parse("EN"), Some(Lang::En));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(Lang::Zh.code(), "zh-CN");
    }

    #[test]
    fn bundle_lookup_and_fallback() {
        let b = Bundle::new();
        assert_eq!(b.get(Lang::Zh, "action.start"), "启动");
        assert_eq!(b.get(Lang::En, "action.start"), "Start");
        assert_eq!(b.get(Lang::En, "missing.key"), "missing.key");
        assert!(b.has("action.stop"));
        assert!(!b.has("nope"));
    }

    #[test]
    fn argument_interpolation() {
        let b = Bundle::new();
        let text = b.get_args(Lang::En, "confirm.delete_instance", &[("name", "survival")]);
        assert!(text.contains("survival"));
        assert!(!text.contains("{name}"));
        let zh = b.get_args(Lang::Zh, "confirm.delete_instance", &[("name", "生存服")]);
        assert!(zh.contains("生存服"));
    }

    #[test]
    fn coverage_and_missing() {
        let b = Bundle::new();
        assert_eq!(b.coverage(Lang::Zh), b.keys().len());
        assert_eq!(b.coverage(Lang::En), b.keys().len());
        assert!(b.missing_for(Lang::En).is_empty());
    }

    #[test]
    fn json_export_is_valid() {
        let b = Bundle::new();
        let text = b.to_json(Lang::En);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["action.start"], "Start");
        assert_eq!(parsed["nav.players"], "Players");
    }

    #[test]
    fn catalog_has_no_duplicate_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for (key, _, _) in CATALOG {
            assert!(seen.insert(*key), "重复键: {key}");
        }
        assert!(CATALOG.len() > 80);
    }

    #[test]
    fn catalog_entries_are_complete() {
        for (key, zh, en) in CATALOG {
            assert!(!key.is_empty());
            assert!(!zh.is_empty(), "{key} 缺少中文");
            assert!(!en.is_empty(), "{key} 缺少英文");
        }
    }
}
