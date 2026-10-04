use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrashKind {
    OutOfMemory,
    PortInUse,
    JavaVersion,
    ModConflict,
    MissingMainClass,
    CorruptWorld,
    PermissionDenied,
    EulaNotAccepted,
    DiskFull,
    JvmCrash,
    KilledBySignal,
    Unknown,
}

impl CrashKind {
    pub fn label(&self) -> &'static str {
        match self {
            CrashKind::OutOfMemory => "内存不足",
            CrashKind::PortInUse => "端口占用",
            CrashKind::JavaVersion => "Java 版本不匹配",
            CrashKind::ModConflict => "模组或插件冲突",
            CrashKind::MissingMainClass => "缺少主类或启动 jar",
            CrashKind::CorruptWorld => "世界存档损坏",
            CrashKind::PermissionDenied => "文件权限不足",
            CrashKind::EulaNotAccepted => "未接受 EULA",
            CrashKind::DiskFull => "磁盘写满",
            CrashKind::JvmCrash => "JVM 崩溃",
            CrashKind::KilledBySignal => "被信号终止",
            CrashKind::Unknown => "未知原因",
        }
    }

    pub fn severity(&self) -> &'static str {
        match self {
            CrashKind::OutOfMemory
            | CrashKind::CorruptWorld
            | CrashKind::JvmCrash
            | CrashKind::DiskFull => "critical",
            CrashKind::PortInUse
            | CrashKind::JavaVersion
            | CrashKind::ModConflict
            | CrashKind::MissingMainClass => "error",
            _ => "warning",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnosis {
    pub kind: CrashKind,
    pub label: String,
    pub severity: String,
    pub evidence: Vec<String>,
    pub hints: Vec<String>,
    pub hs_err_path: Option<String>,
}

impl Diagnosis {
    pub fn summary(&self) -> String {
        let mut s = format!("{} ({})", self.label, self.severity);
        if !self.evidence.is_empty() {
            s.push_str(" | ");
            s.push_str(&self.evidence.join(" ; "));
        }
        s
    }

    pub fn actionable(&self) -> bool {
        !self.hints.is_empty()
    }
}

struct Pattern {
    kind: CrashKind,
    needles: &'static [&'static str],
    hints: &'static [&'static str],
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        kind: CrashKind::OutOfMemory,
        needles: &[
            "java.lang.OutOfMemoryError",
            "OutOfMemoryError",
            "unable to create native thread",
            "There is insufficient memory for the Java Runtime Environment",
            "Could not reserve enough space for object heap",
        ],
        hints: &[
            "调高实例内存上限, 或降低 Xmx 与模组数量",
            "检查宿主机是否被其他实例挤占",
        ],
    },
    Pattern {
        kind: CrashKind::PortInUse,
        needles: &[
            "Address already in use",
            "java.net.BindException",
            "Failed to bind to port",
            "Perhaps a server is already running on that port",
        ],
        hints: &[
            "换一个 server-port, 或先停止占用该端口的进程",
            "确认没有重复启动同一个实例",
        ],
    },
    Pattern {
        kind: CrashKind::JavaVersion,
        needles: &[
            "UnsupportedClassVersionError",
            "class file version",
            "requires Java",
            "has been compiled by a more recent version",
            "Unsupported Java",
        ],
        hints: &[
            "把实例 Java 大版本调到服务器要求的值",
            "重新下载对应 Temurin 运行时",
        ],
    },
    Pattern {
        kind: CrashKind::ModConflict,
        needles: &[
            "Duplicate mod",
            "mixin apply failed",
            "Mixin apply failed",
            "Incompatible mod set",
            "Mod resolution failed",
            "NoSuchMethodError",
            "NoClassDefFoundError",
        ],
        hints: &[
            "用依赖解析功能检查缺失与冲突的模组",
            "逐个禁用最近新增的模组定位问题",
        ],
    },
    Pattern {
        kind: CrashKind::MissingMainClass,
        needles: &[
            "Could not find or load main class",
            "no main manifest attribute",
            "Unable to access jarfile",
            "jar file not found",
        ],
        hints: &[
            "确认 server.jar 存在且为可执行服务端核心",
            "在版本页重新安装或导入核心",
        ],
    },
    Pattern {
        kind: CrashKind::CorruptWorld,
        needles: &[
            "corrupted chunk",
            "Failed to load level",
            "Exception loading level",
            "region file",
            "unable to read level.dat",
        ],
        hints: &["从最近一次备份恢复世界", "删除损坏的 region 文件后重新生成"],
    },
    Pattern {
        kind: CrashKind::PermissionDenied,
        needles: &[
            "Permission denied",
            "AccessDeniedException",
            "Operation not permitted",
            "cannot open",
        ],
        hints: &[
            "检查实例目录的属主与读写权限",
            "确认进程用户对工作目录有写权限",
        ],
    },
    Pattern {
        kind: CrashKind::EulaNotAccepted,
        needles: &["You need to agree to the EULA", "eula.txt"],
        hints: &["在实例页勾选并保存 EULA 同意"],
    },
    Pattern {
        kind: CrashKind::DiskFull,
        needles: &[
            "No space left on device",
            "There is not enough space on the disk",
        ],
        hints: &["清理旧备份或日志", "扩容所在分区"],
    },
];

pub fn classify_exit(code: Option<i32>) -> Option<CrashKind> {
    match code {
        Some(137) | Some(-9) | Some(143) | Some(-15) => Some(CrashKind::KilledBySignal),
        Some(134) | Some(-6) | Some(139) | Some(-11) => Some(CrashKind::JvmCrash),
        _ => None,
    }
}

pub fn scan_text(text: &str) -> (Option<CrashKind>, Vec<String>, Vec<String>) {
    let mut kind: Option<CrashKind> = None;
    let mut evidence = Vec::new();
    let mut hints = Vec::new();
    for pattern in PATTERNS {
        for needle in pattern.needles {
            if text.contains(needle) {
                evidence.push(needle.to_string());
                if kind.is_none() {
                    kind = Some(pattern.kind);
                    for h in pattern.hints {
                        hints.push(h.to_string());
                    }
                }
                break;
            }
        }
    }
    (kind, evidence, hints)
}

pub fn find_hs_err(workdir: &str) -> Option<PathBuf> {
    let dir = Path::new(workdir);
    let entries = std::fs::read_dir(dir).ok()?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("hs_err_pid") && name.ends_with(".log") {
            if let Ok(meta) = entry.metadata() {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                    newest = Some((modified, entry.path()));
                }
            }
        }
    }
    newest.map(|(_, p)| p)
}

pub fn summarize_hs_err(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines().take(40) {
        let t = line.trim();
        if t.contains("fatal error")
            || t.contains("insufficient memory")
            || t.contains("SIGSEGV")
            || t.contains("Problematic frame")
            || t.contains("JRE version")
            || t.contains("Java VM")
        {
            out.push(t.to_string());
        }
        if out.len() >= 6 {
            break;
        }
    }
    out
}

pub fn diagnose(workdir: &str, exit_code: Option<i32>, stderr_tail: &str) -> Diagnosis {
    let (text_kind, mut evidence, mut hints) = scan_text(stderr_tail);
    let exit_kind = classify_exit(exit_code);
    let hs_err = find_hs_err(workdir);
    let mut kind = text_kind.or(exit_kind).unwrap_or(CrashKind::Unknown);
    if kind == CrashKind::Unknown {
        if let Some(path) = hs_err.as_ref() {
            let lines = summarize_hs_err(path);
            if !lines.is_empty() {
                kind = CrashKind::JvmCrash;
                evidence.extend(lines);
                hints.push("查看 hs_err 日志中的 Problematic frame 定位崩溃模组".into());
            }
        }
    }
    if let Some(code) = exit_code {
        evidence.push(format!("exit={code}"));
    }
    if hints.is_empty() {
        hints.push("查看完整控制台日志定位退出原因".into());
    }
    Diagnosis {
        kind,
        label: kind.label().to_string(),
        severity: kind.severity().to_string(),
        evidence,
        hints,
        hs_err_path: hs_err.map(|p| p.to_string_lossy().replace('\\', "/")),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
    pub base_delay_secs: u64,
    pub max_delay_secs: u64,
    pub reset_after_secs: u64,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 5,
            base_delay_secs: 5,
            max_delay_secs: 300,
            reset_after_secs: 900,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RestartState {
    pub attempts: u32,
    pub last_at: Option<std::time::Instant>,
    pub given_up: bool,
}

impl RestartState {
    pub fn record_success(&mut self) {
        self.attempts = 0;
        self.given_up = false;
        self.last_at = Some(std::time::Instant::now());
    }

    pub fn should_attempt(&mut self, policy: &RestartPolicy) -> bool {
        if !policy.enabled || policy.max_attempts == 0 {
            self.given_up = true;
            return false;
        }
        let now = std::time::Instant::now();
        if let Some(last) = self.last_at {
            if now.duration_since(last).as_secs() >= policy.reset_after_secs {
                self.attempts = 0;
                self.given_up = false;
            }
        }
        if self.attempts >= policy.max_attempts {
            self.given_up = true;
            return false;
        }
        self.attempts += 1;
        self.last_at = Some(now);
        true
    }

    pub fn next_delay(&self, policy: &RestartPolicy) -> Duration {
        if self.attempts == 0 {
            return Duration::ZERO;
        }
        let exp = self.attempts.saturating_sub(1).min(20);
        let factor = 1u64 << exp;
        let secs = policy
            .base_delay_secs
            .saturating_mul(factor)
            .min(policy.max_delay_secs);
        Duration::from_secs(secs)
    }
}

pub fn restart_policy_for(kind: CrashKind) -> RestartPolicy {
    let mut policy = RestartPolicy::default();
    match kind {
        CrashKind::PortInUse | CrashKind::EulaNotAccepted | CrashKind::JavaVersion => {
            policy.max_attempts = 1;
            policy.base_delay_secs = 10;
        }
        CrashKind::OutOfMemory => {
            policy.max_attempts = 3;
            policy.base_delay_secs = 30;
        }
        CrashKind::CorruptWorld | CrashKind::DiskFull => {
            policy.max_attempts = 0;
            policy.enabled = false;
        }
        _ => {}
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_oom() {
        let d = diagnose(
            "/nonexistent",
            Some(1),
            "java.lang.OutOfMemoryError: Java heap space",
        );
        assert_eq!(d.kind, CrashKind::OutOfMemory);
        assert_eq!(d.severity, "critical");
        assert!(d.actionable());
    }

    #[test]
    fn classifies_port_conflict() {
        let d = diagnose(
            "/nonexistent",
            Some(1),
            "java.net.BindException: Address already in use",
        );
        assert_eq!(d.kind, CrashKind::PortInUse);
        assert!(d.evidence.iter().any(|e| e.contains("already in use")));
    }

    #[test]
    fn classifies_java_version() {
        let d = diagnose(
            "/nonexistent",
            None,
            "java.lang.UnsupportedClassVersionError: class file version 65.0",
        );
        assert_eq!(d.kind, CrashKind::JavaVersion);
    }

    #[test]
    fn classifies_mod_conflict() {
        let d = diagnose(
            "/nonexistent",
            None,
            "Mixin apply failed for mod fabric-api, Duplicate mod id",
        );
        assert_eq!(d.kind, CrashKind::ModConflict);
    }

    #[test]
    fn exit_code_signal_mapping() {
        assert_eq!(classify_exit(Some(137)), Some(CrashKind::KilledBySignal));
        assert_eq!(classify_exit(Some(-11)), Some(CrashKind::JvmCrash));
        assert_eq!(classify_exit(Some(0)), None);
        assert_eq!(classify_exit(None), None);
    }

    #[test]
    fn unknown_when_no_signal() {
        let d = diagnose("/nonexistent", Some(3), "some random output");
        assert_eq!(d.kind, CrashKind::Unknown);
        assert!(!d.hints.is_empty());
    }

    #[test]
    fn hs_err_detection() {
        let dir = std::env::temp_dir().join(format!("ck-hs-{}", crate::crypto::random_token(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hs_err_pid1234.log");
        std::fs::write(
            &file,
            "A fatal error has been detected by the Java Runtime Environment:\nSIGSEGV (0xb) at pc=0x00007f\nJRE version: OpenJDK 21\nProblematic frame:\nC  libfoo.so\n",
        )
        .unwrap();
        let found = find_hs_err(dir.to_str().unwrap()).unwrap();
        assert_eq!(found, file);
        let lines = summarize_hs_err(&found);
        assert!(lines.iter().any(|l| l.contains("fatal error")));
        let d = diagnose(dir.to_str().unwrap(), None, "");
        assert_eq!(d.kind, CrashKind::JvmCrash);
        assert!(d.hs_err_path.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restart_backoff_grows_and_caps() {
        let policy = RestartPolicy {
            enabled: true,
            max_attempts: 5,
            base_delay_secs: 2,
            max_delay_secs: 30,
            reset_after_secs: 900,
        };
        let mut state = RestartState::default();
        assert!(state.should_attempt(&policy));
        assert_eq!(state.next_delay(&policy), Duration::from_secs(2));
        assert!(state.should_attempt(&policy));
        assert_eq!(state.next_delay(&policy), Duration::from_secs(4));
        assert!(state.should_attempt(&policy));
        assert_eq!(state.next_delay(&policy), Duration::from_secs(8));
        assert!(state.should_attempt(&policy));
        assert_eq!(state.next_delay(&policy), Duration::from_secs(16));
        assert!(state.should_attempt(&policy));
        assert_eq!(state.next_delay(&policy), Duration::from_secs(30));
        assert!(!state.should_attempt(&policy));
        assert!(state.given_up);
        state.record_success();
        assert!(state.should_attempt(&policy));
    }

    #[test]
    fn policy_disabled_for_corrupt_world() {
        let p = restart_policy_for(CrashKind::CorruptWorld);
        assert!(!p.enabled);
        let mut st = RestartState::default();
        assert!(!st.should_attempt(&p));
        let p2 = restart_policy_for(CrashKind::PortInUse);
        assert_eq!(p2.max_attempts, 1);
    }
}
