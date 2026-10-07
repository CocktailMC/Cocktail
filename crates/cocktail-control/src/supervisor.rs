//! 通用本地服务生命周期管理器。
//!
//! `ServiceSupervisor` 统一管理本地子进程服务的启动 / 停止 / 监督 / 失败重启 /
//! 依赖拓扑 / 日志轮转 / 资源监控 / 权限边界。`cocktail-init` 是第一个
//! `Transport::Ipc` 服务。
//!
//! 设计原则：声明式 + 监控执行。不使用 Windows Job Object / Unix rlimit / unsafe。
//! 资源限制仅通过周期采样 + 超限动作（Warn / Restart）逼近。

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tracing::{debug, info, warn};

use cocktail_shared::logfmt::{self, Badge};
use cocktail_shared::logging::{self, LiveLine};

use crate::init_client::InitClient;

/// 服务名（也是注册 key 与日志文件名）。
pub type ServiceName = String;

/// 服务短名：`cocktail-init` → `init`，用于派生事件名前缀（如 `init.waiting`）。
fn short_name(name: &str) -> String {
    name.strip_prefix("cocktail-").unwrap_or(name).to_string()
}

/// 退出码文本，未知用 `-`。
fn code_text(code: Option<i32>) -> String {
    code.map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string())
}

// ---------------------------------------------------------------------------
// A. 纯逻辑层
// ---------------------------------------------------------------------------

/// 重启策略。
#[derive(Debug, Clone)]
pub enum RestartPolicy {
    /// 从不重启。非 0 退出即 Failed，0 退出即 Stopped。
    Never,
    /// 仅在失败（非 0 退出）时重启，attempts 达到 max_attempts 后放弃。
    OnFailure {
        max_attempts: u32,
        backoff_ms: u64,
        max_backoff_ms: u64,
    },
    /// 总是重启（0 退出也重启）。
    Always {
        backoff_ms: u64,
        max_backoff_ms: u64,
    },
}

/// 第 `attempt` 次（1-based）重启的退避时长，指数增长并封顶 `max_backoff_ms`。
/// `Never` 返回 None。
pub fn backoff_delay(policy: &RestartPolicy, attempt: u32) -> Option<Duration> {
    let (base, max) = match policy {
        RestartPolicy::Never => return None,
        RestartPolicy::OnFailure {
            backoff_ms,
            max_backoff_ms,
            ..
        } => (*backoff_ms, *max_backoff_ms),
        RestartPolicy::Always {
            backoff_ms,
            max_backoff_ms,
        } => (*backoff_ms, *max_backoff_ms),
    };
    // factor = 2^(attempt-1)，attempt 从 1 起。attempt=0 时按第 1 次处理。
    let exp = attempt.saturating_sub(1).min(63);
    let factor = 1u64.checked_shl(exp).unwrap_or(u64::MAX);
    let ms = base.saturating_mul(factor).min(max);
    Some(Duration::from_millis(ms))
}

/// 拓扑排序 + 环检测。返回包含 `target` 及其全部（传递）依赖的启动顺序，
/// 依赖排在被依赖者之前。
pub fn startup_order(
    specs: &HashMap<String, Arc<ServiceSpec>>,
    target: &str,
) -> anyhow::Result<Vec<String>> {
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    visit(specs, target, &mut order, &mut visited, &mut stack)?;
    Ok(order)
}

/// 停止顺序：`startup_order` 的反转（先停依赖方，再停被依赖方）。
pub fn shutdown_order(
    specs: &HashMap<String, Arc<ServiceSpec>>,
    target: &str,
) -> anyhow::Result<Vec<String>> {
    let mut order = startup_order(specs, target)?;
    order.reverse();
    Ok(order)
}

/// 累加式拓扑访问。`stack` 保存当前 DFS 路径用于环检测。
fn visit(
    specs: &HashMap<String, Arc<ServiceSpec>>,
    name: &str,
    order: &mut Vec<String>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> anyhow::Result<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if stack.iter().any(|n| n == name) {
        anyhow::bail!(
            "service dependency cycle detected: {} -> {name}",
            stack.join(" -> ")
        );
    }
    let spec = specs
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("service '{name}' is not registered"))?;
    stack.push(name.to_string());
    for dep in &spec.depends_on {
        visit(specs, dep, order, visited, stack)?;
    }
    stack.pop();
    visited.insert(name.to_string());
    order.push(name.to_string());
    Ok(())
}

/// 日志轮转目标路径：index=0 为主文件，index>=1 为 `<path>.<index>`。
pub fn rotated_path(main: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return main.to_path_buf();
    }
    let mut s = main.as_os_str().to_os_string();
    s.push(format!(".{index}"));
    PathBuf::from(s)
}

/// 路径是否落在任一白名单根目录之下（尽量用 canonicalize，失败则退化为字面比较）。
fn under_any(path: &Path, roots: &[PathBuf]) -> bool {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    roots.iter().any(|root| {
        let r = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        p.starts_with(&r)
    })
}

// ---------------------------------------------------------------------------
// B. 数据类型
// ---------------------------------------------------------------------------

/// 子进程 IO 传输方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// JSON-RPC over stdin/stdout（cocktail-init）。stdout 留给 RPC，日志从
    /// stderr 采集。supervisor 会构造 `InitClient` 供 IPC 调用。
    Ipc,
    /// 普通进程：stdout + stderr 都写入日志文件。
    Log,
}

/// 资源超限时的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitAction {
    Warn,
    Restart,
}

/// 声明式资源限制。全 None 表示不限制。
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<f32>,
    pub sample_interval: Duration,
    pub on_exceed: LimitAction,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: None,
            max_cpu_percent: None,
            sample_interval: Duration::from_secs(10),
            on_exceed: LimitAction::Warn,
        }
    }
}

/// 权限边界。空白名单 = 不限制；`env_denylist` 默认空。
#[derive(Debug, Clone, Default)]
pub struct PermissionBoundary {
    /// 可执行文件必须落在这些根目录之下（空 = 不限制）。
    pub allowed_bin_roots: Vec<PathBuf>,
    /// cwd 必须落在这些根目录之下（空 = 不限制）。
    pub allowed_workdir_roots: Vec<PathBuf>,
    /// spawn 前从继承环境中剔除的变量名。
    pub env_denylist: Vec<String>,
}

impl PermissionBoundary {
    /// 校验可执行文件与 cwd 落在白名单内。`cwd` 为空则跳过工作目录校验。
    pub fn validate(&self, program: &Path, cwd: Option<&Path>) -> anyhow::Result<()> {
        if !self.allowed_bin_roots.is_empty() && !under_any(program, &self.allowed_bin_roots) {
            anyhow::bail!("program {} is outside allowed_bin_roots", program.display());
        }
        if let Some(cwd) = cwd {
            if !self.allowed_workdir_roots.is_empty()
                && !under_any(cwd, &self.allowed_workdir_roots)
            {
                anyhow::bail!("workdir {} is outside allowed_workdir_roots", cwd.display());
            }
        }
        Ok(())
    }
}

/// 服务声明。
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub name: ServiceName,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub transport: Transport,
    pub depends_on: Vec<ServiceName>,
    pub restart: RestartPolicy,
    pub limits: ResourceLimits,
    pub boundary: PermissionBoundary,
    /// None 用默认 `data/logs/services`。
    pub log_dir: Option<PathBuf>,
    pub log_max_bytes: u64,
    pub log_keep: usize,
    pub stop_timeout: Duration,
}

/// 服务状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Backoff,
    Failed,
}

/// 对外状态快照。
#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub name: ServiceName,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub restarts: u64,
    pub last_exit_code: Option<i32>,
    pub last_error: Option<String>,
    pub since: DateTime<Utc>,
}

/// 退出后的重启决策。
enum RestartDecision {
    NoRestart { failed: bool },
    Restart { delay: Duration },
}

/// 依据策略决定是否重启。`restarts` 为已发生的重启次数。
fn decide_restart(policy: &RestartPolicy, code: Option<i32>, restarts: u64) -> RestartDecision {
    let ok = matches!(code, Some(0));
    match policy {
        RestartPolicy::Never => RestartDecision::NoRestart { failed: !ok },
        RestartPolicy::OnFailure { max_attempts, .. } => {
            if ok {
                RestartDecision::NoRestart { failed: false }
            } else if restarts < u64::from(*max_attempts) {
                let attempt = restarts as u32 + 1;
                RestartDecision::Restart {
                    delay: backoff_delay(policy, attempt).unwrap_or(Duration::ZERO),
                }
            } else {
                RestartDecision::NoRestart { failed: true }
            }
        }
        RestartPolicy::Always { .. } => {
            let attempt = restarts as u32 + 1;
            RestartDecision::Restart {
                delay: backoff_delay(policy, attempt).unwrap_or(Duration::ZERO),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// C. 运行时
// ---------------------------------------------------------------------------

/// 停止请求：watcher 完成优雅停止后通过 `done` 通知调用方。
struct StopRequest {
    done: oneshot::Sender<()>,
}

/// 单服务运行时状态。
struct ServiceRuntime {
    name: ServiceName,
    state: ServiceState,
    pid: Option<u32>,
    restarts: u64,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    since: DateTime<Utc>,
    /// 每次 spawn 自增，用于忽略过期 watcher / monitor 的回调。
    generation: u64,
    /// 正在主动停止：抑制 respawn。
    stopping: bool,
    /// Ipc 传输的 IPC client（Log 传输为 None）。
    client: Option<Arc<InitClient>>,
    /// 与 watcher 通信的停止通道；watcher 未运行时为 None。
    stop_tx: Option<mpsc::Sender<StopRequest>>,
}

impl ServiceRuntime {
    fn new(name: ServiceName) -> Self {
        Self {
            name,
            state: ServiceState::Stopped,
            pid: None,
            restarts: 0,
            last_exit_code: None,
            last_error: None,
            since: Utc::now(),
            generation: 0,
            stopping: false,
            client: None,
            stop_tx: None,
        }
    }

    fn snapshot(&self) -> ServiceStatus {
        ServiceStatus {
            name: self.name.clone(),
            state: self.state,
            pid: self.pid,
            restarts: self.restarts,
            last_exit_code: self.last_exit_code,
            last_error: self.last_error.clone(),
            since: self.since,
        }
    }
}

/// 通用本地服务管理器。
pub struct ServiceSupervisor {
    specs: RwLock<HashMap<String, Arc<ServiceSpec>>>,
    runtime: RwLock<HashMap<String, ServiceRuntime>>,
    default_log_dir: PathBuf,
    me: OnceLock<Weak<ServiceSupervisor>>,
}

impl ServiceSupervisor {
    /// 创建 supervisor（返回 Arc，内部自持弱引用供 async 任务获取所有权）。
    pub fn new() -> Arc<Self> {
        let sup = Arc::new(Self {
            specs: RwLock::new(HashMap::new()),
            runtime: RwLock::new(HashMap::new()),
            default_log_dir: PathBuf::from("data").join("logs").join("services"),
            me: OnceLock::new(),
        });
        let _ = sup.me.set(Arc::downgrade(&sup));
        sup
    }

    fn arc(&self) -> Option<Arc<Self>> {
        self.me.get().and_then(|w| w.upgrade())
    }

    /// 注册服务；检测重复名 + 依赖不存在 + 依赖环，失败返回 Err。
    pub async fn register(&self, spec: ServiceSpec) -> anyhow::Result<()> {
        let name = spec.name.clone();
        {
            let specs = self.specs.read().await;
            if specs.contains_key(&name) {
                anyhow::bail!("service '{name}' already registered");
            }
            for dep in &spec.depends_on {
                if dep == &name {
                    anyhow::bail!("service '{name}' depends on itself");
                }
                if !specs.contains_key(dep) {
                    anyhow::bail!("service '{name}' depends on unregistered service '{dep}'");
                }
            }
        }
        let mut specs = self.specs.write().await;
        specs.insert(name.clone(), Arc::new(spec));
        // 环检测：借助 startup_order 遍历，出错则回滚注册。
        if let Err(e) = startup_order(&specs, &name) {
            specs.remove(&name);
            return Err(e);
        }
        drop(specs);
        self.runtime
            .write()
            .await
            .insert(name.clone(), ServiceRuntime::new(name));
        Ok(())
    }

    /// 按依赖拓扑顺序启动 `target` 及其依赖。已在 Running / Starting 的跳过。
    pub async fn start(&self, target: &str) -> anyhow::Result<()> {
        let order = {
            let specs = self.specs.read().await;
            startup_order(&specs, target)?
        };
        for name in order {
            let skip = {
                let rt = self.runtime.read().await;
                matches!(
                    rt.get(&name).map(|r| r.state),
                    Some(ServiceState::Running) | Some(ServiceState::Starting)
                )
            };
            if skip {
                continue;
            }
            self.spawn_and_watch(&name).await?;
        }
        Ok(())
    }

    /// 反向依赖顺序停止 `target`（先停 target，再停其依赖）。
    pub async fn stop(&self, target: &str) -> anyhow::Result<()> {
        let order = {
            let specs = self.specs.read().await;
            shutdown_order(&specs, target)?
        };
        for name in order {
            self.stop_one(&name).await;
        }
        Ok(())
    }

    /// 停止再启动 `target`（含依赖链）。
    pub async fn restart(&self, target: &str) -> anyhow::Result<()> {
        self.stop(target).await?;
        self.start(target).await
    }

    /// 状态查询。
    pub async fn status(&self, name: &str) -> Option<ServiceStatus> {
        let rt = self.runtime.read().await;
        rt.get(name).map(|r| r.snapshot())
    }

    /// 全部服务状态。
    pub async fn list(&self) -> Vec<ServiceStatus> {
        let rt = self.runtime.read().await;
        rt.values().map(|r| r.snapshot()).collect()
    }

    /// 全部停止（control 退出时调用），按全局依赖反向序。
    pub async fn shutdown_all(&self) {
        let order = self.all_shutdown_order().await;
        for name in order {
            self.stop_one(&name).await;
        }
    }

    /// IPC 调用（仅 `Transport::Ipc` 服务，当前即 cocktail-init）。
    pub async fn ipc_call<P: Serialize>(
        &self,
        service: &str,
        method: &str,
        params: P,
    ) -> io::Result<serde_json::Value> {
        let client = self.ipc_client(service).await?;
        client.call(method, params).await
    }

    /// IPC 拉取 master key（32 字节）。
    pub async fn ipc_get_master_key(&self, service: &str) -> io::Result<Vec<u8>> {
        let client = self.ipc_client(service).await?;
        client.get_master_key().await
    }

    /// 订阅 init 推送的事件。返回 `None` 表示该服务未启动 / 无 IPC client。
    ///
    /// 订阅与具体一次 spawn 绑定：服务重启后旧 receiver 会收到 `Closed`，
    /// 调用方需重新调用本方法重建订阅。
    pub async fn subscribe_events(
        &self,
        service: &str,
    ) -> Option<tokio::sync::broadcast::Receiver<cocktail_shared::proto::Event>> {
        let client = self.ipc_client(service).await.ok()?;
        Some(client.subscribe_events())
    }

    pub(crate) async fn ipc_client(&self, service: &str) -> io::Result<Arc<InitClient>> {
        let rt = self.runtime.read().await;
        rt.get(service)
            .and_then(|r| r.client.clone())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!("service '{service}' IPC not available (not running or restarting)"),
                )
            })
    }

    async fn spec(&self, name: &str) -> Option<Arc<ServiceSpec>> {
        self.specs.read().await.get(name).cloned()
    }

    /// 全局停止顺序（所有服务拓扑序反转）。
    async fn all_shutdown_order(&self) -> Vec<String> {
        let specs = self.specs.read().await;
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        for name in specs.keys() {
            let mut stack = Vec::new();
            // 注册时已校验，正常不会出错。
            let _ = visit(&specs, name, &mut order, &mut visited, &mut stack);
        }
        order.reverse();
        order
    }

    /// spawn 子进程并安装运行时、watcher、monitor。
    async fn spawn_and_watch(&self, name: &str) -> anyhow::Result<()> {
        let spec = self
            .spec(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("service '{name}' is not registered"))?;

        {
            let mut rt = self.runtime.write().await;
            if let Some(r) = rt.get_mut(name) {
                r.state = ServiceState::Starting;
                r.stopping = false;
                r.last_error = None;
                r.since = Utc::now();
            }
        }

        // 权限边界校验
        if let Err(e) = spec.boundary.validate(&spec.program, spec.cwd.as_deref()) {
            self.mark_failed(name, Some(format!("permission boundary rejected: {e}")))
                .await;
            logging::emit(
                Badge::Fail,
                name,
                &format!("{}.failed", short_name(name)),
                vec![("error".to_string(), e.to_string())],
            );
            self.cascade_failure(name, "permission boundary rejected")
                .await;
            return Err(anyhow::anyhow!(
                "service '{name}' boundary check failed: {e}"
            ));
        }

        // 构建命令：剔除 denylist、注入声明式 env。
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args).kill_on_drop(false);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for key in &spec.boundary.env_denylist {
            cmd.env_remove(key);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        crate::wincompat::hide_console(&mut cmd);

        let log_path = self.log_path(&spec);

        // 统一日志：WAIT → 实时 starting 行 → DONE started
        let short = short_name(name);
        let started = Instant::now();
        logging::emit(
            Badge::Wait,
            name,
            &format!("{short}.waiting"),
            vec![("parent".to_string(), "control".to_string())],
        );
        let live = LiveLine::begin(name.to_string(), format!("{short}.starting"), Vec::new());

        let spawned: anyhow::Result<(Child, Option<Arc<InitClient>>)> = async {
            match spec.transport {
                Transport::Ipc => {
                    let (client, child, stderr) = InitClient::spawn(&mut cmd)
                        .map_err(|e| anyhow::anyhow!("spawn {}: {e}", spec.program.display()))?;
                    let writer = Arc::new(Mutex::new(
                        LogWriter::open(log_path.clone(), spec.log_max_bytes, spec.log_keep)
                            .await?,
                    ));
                    // init 的 stderr 是日志通道
                    tokio::spawn(pipe_to_log(stderr, writer));
                    Ok((child, Some(client)))
                }
                Transport::Log => {
                    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
                    let mut child = cmd
                        .spawn()
                        .map_err(|e| anyhow::anyhow!("spawn {}: {e}", spec.program.display()))?;
                    let writer = Arc::new(Mutex::new(
                        LogWriter::open(log_path.clone(), spec.log_max_bytes, spec.log_keep)
                            .await?,
                    ));
                    if let Some(out) = child.stdout.take() {
                        tokio::spawn(pipe_to_log(out, Arc::clone(&writer)));
                    }
                    if let Some(err) = child.stderr.take() {
                        tokio::spawn(pipe_to_log(err, Arc::clone(&writer)));
                    }
                    Ok((child, None))
                }
            }
        }
        .await;

        let (child, client) = match spawned {
            Ok(v) => v,
            Err(e) => {
                live.fail(&format!("{short}.failed"), &format!("{e:#}"));
                return Err(e);
            }
        };

        let pid = child.id();
        let (stop_tx, stop_rx) = mpsc::channel(1);
        let generation = {
            let mut rt = self.runtime.write().await;
            let r = rt
                .get_mut(name)
                .ok_or_else(|| anyhow::anyhow!("service '{name}' vanished during spawn"))?;
            r.generation += 1;
            r.state = ServiceState::Running;
            r.pid = pid;
            r.client = client.clone();
            r.stop_tx = Some(stop_tx);
            r.since = Utc::now();
            r.generation
        };

        live.done(
            &format!("{short}.started"),
            vec![
                (
                    "pid".to_string(),
                    pid.map(|p| p.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "duration".to_string(),
                    logfmt::human_duration(started.elapsed()),
                ),
            ],
        );

        // 资源监控（仅在声明了限制时启用）
        if let Some(pid) = pid {
            if spec.limits.max_memory_mb.is_some() || spec.limits.max_cpu_percent.is_some() {
                self.spawn_monitor(name.to_string(), Arc::clone(&spec), pid, generation);
            }
        }

        self.spawn_watcher(
            name.to_string(),
            Arc::clone(&spec),
            child,
            stop_rx,
            generation,
            client,
        );
        Ok(())
    }

    /// 单个服务的停止（置 Stopping → watcher 优雅停止 → Stopped）。
    async fn stop_one(&self, name: &str) {
        let stop_tx = {
            let mut rt = self.runtime.write().await;
            let Some(r) = rt.get_mut(name) else {
                return;
            };
            if matches!(r.state, ServiceState::Stopped | ServiceState::Failed) {
                return;
            }
            r.stopping = true;
            r.state = ServiceState::Stopping;
            r.since = Utc::now();
            r.stop_tx.take()
        };

        if let Some(tx) = stop_tx {
            let (done_tx, done_rx) = oneshot::channel();
            if tx.send(StopRequest { done: done_tx }).await.is_ok() {
                let _ = tokio::time::timeout(Duration::from_secs(60), done_rx).await;
            }
        }

        // 兜底：确保进入 Stopped（watcher 可能已先行退出/失败）。
        let mut rt = self.runtime.write().await;
        if let Some(r) = rt.get_mut(name) {
            r.state = ServiceState::Stopped;
            r.pid = None;
            r.client = None;
            r.stop_tx = None;
            r.stopping = false;
            r.since = Utc::now();
        }
    }

    /// 记录 Failed 状态。
    async fn mark_failed(&self, name: &str, err: Option<String>) {
        let mut rt = self.runtime.write().await;
        if let Some(r) = rt.get_mut(name) {
            r.state = ServiceState::Failed;
            r.last_error = err;
            r.pid = None;
            r.since = Utc::now();
        }
    }

    /// 依赖级联：`failed` 进入 Failed 后，其所有（传递）下游也置 Failed。
    async fn cascade_failure(&self, failed: &str, reason: &str) {
        let specs = self.specs.read().await;
        let mut queue = vec![failed.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(failed.to_string());
        let mut victims = Vec::new();
        while let Some(node) = queue.pop() {
            for (cand, s) in specs.iter() {
                if s.depends_on.iter().any(|d| d == &node) && !seen.contains(cand) {
                    seen.insert(cand.clone());
                    queue.push(cand.clone());
                    victims.push(cand.clone());
                }
            }
        }
        drop(specs);
        if victims.is_empty() {
            return;
        }
        let mut rt = self.runtime.write().await;
        for v in &victims {
            if let Some(r) = rt.get_mut(v) {
                if r.state != ServiceState::Stopped {
                    r.state = ServiceState::Failed;
                    r.last_error = Some(format!("{reason} (upstream '{failed}' failed)"));
                    r.stopping = true; // 抑制下游 respawn
                    r.since = Utc::now();
                }
            }
        }
        warn!(
            upstream = %failed,
            count = victims.len(),
            "cascading failure to downstream services"
        );
    }

    /// 子进程退出后的处理：决策重启或落 Failed/Stopped。
    async fn handle_child_exit(
        &self,
        name: &str,
        spec: &ServiceSpec,
        generation: u64,
        code: Option<i32>,
    ) {
        let mut schedule: Option<Duration> = None;
        let mut failed = false;
        let attempts;
        {
            let mut rt = self.runtime.write().await;
            let Some(r) = rt.get_mut(name) else {
                return;
            };
            // 已被新实例取代 / 过期 watcher，忽略。
            if r.generation != generation {
                return;
            }
            r.pid = None;
            r.client = None;
            r.stop_tx = None;
            r.last_exit_code = code;
            if r.stopping {
                r.state = ServiceState::Stopped;
                r.stopping = false;
                r.since = Utc::now();
                logging::emit(
                    Badge::Done,
                    name,
                    &format!("{}.stopped", short_name(name)),
                    vec![("code".to_string(), code_text(code))],
                );
                return;
            }
            match decide_restart(&spec.restart, code, r.restarts) {
                RestartDecision::NoRestart { failed: f } => {
                    failed = f;
                    attempts = r.restarts;
                    r.state = if f {
                        ServiceState::Failed
                    } else {
                        ServiceState::Stopped
                    };
                    if f {
                        r.last_error = Some(format!("exited with code {code:?}"));
                    }
                    r.since = Utc::now();
                }
                RestartDecision::Restart { delay } => {
                    r.restarts += 1;
                    attempts = r.restarts;
                    r.state = ServiceState::Backoff;
                    r.since = Utc::now();
                    schedule = Some(delay);
                }
            }
        }

        match schedule {
            Some(delay) => {
                logging::emit(
                    Badge::Warn,
                    name,
                    &format!("{}.restarting", short_name(name)),
                    vec![
                        ("attempt".to_string(), attempts.to_string()),
                        ("delay".to_string(), logfmt::human_duration(delay)),
                    ],
                );
                let Some(sup) = self.arc() else { return };
                let name = name.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    sup.respawn_after_backoff(&name, generation).await;
                });
            }
            None => {
                if failed {
                    logging::emit(
                        Badge::Fail,
                        name,
                        &format!("{}.failed", short_name(name)),
                        vec![("attempts".to_string(), attempts.to_string())],
                    );
                    self.cascade_failure(name, "upstream failed").await;
                } else {
                    logging::emit(
                        Badge::Done,
                        name,
                        &format!("{}.stopped", short_name(name)),
                        vec![("code".to_string(), code_text(code))],
                    );
                }
            }
        }
    }

    /// 退避到期后重启服务（若期间未被 stop）。
    async fn respawn_after_backoff(&self, name: &str, generation: u64) {
        {
            let rt = self.runtime.read().await;
            match rt.get(name) {
                Some(r)
                    if r.generation == generation
                        && r.state == ServiceState::Backoff
                        && !r.stopping => {}
                _ => return,
            }
        }
        info!(service = %name, "backoff elapsed; respawning service");
        if let Err(e) = self.spawn_and_watch(name).await {
            warn!(service = %name, error = %e, "respawn failed");
        }
    }

    /// 资源超限触发的单服务重启（计一次 restart）。
    async fn restart_single(&self, name: &str) {
        self.stop_one(name).await;
        {
            let mut rt = self.runtime.write().await;
            if let Some(r) = rt.get_mut(name) {
                r.restarts += 1;
            }
        }
        if let Err(e) = self.spawn_and_watch(name).await {
            warn!(service = %name, error = %e, "restart failed");
        }
    }

    /// 启动 watcher：等待子进程退出或主动停止请求。
    fn spawn_watcher(
        &self,
        name: String,
        spec: Arc<ServiceSpec>,
        mut child: Child,
        mut stop_rx: mpsc::Receiver<StopRequest>,
        generation: u64,
        client: Option<Arc<InitClient>>,
    ) {
        let Some(sup) = self.arc() else { return };
        tokio::spawn(async move {
            // 注意：`child.wait()` 在 select! 期间可变借用 child，因此两个分支的
            // 处理逻辑都放在 select! 之外执行，避免同时可变借用冲突。
            let mut stop_req: Option<Option<StopRequest>> = None;
            let mut exit_code: Option<Option<i32>> = None;
            tokio::select! {
                req = stop_rx.recv() => {
                    stop_req = Some(req);
                }
                status = child.wait() => {
                    exit_code = Some(status.ok().and_then(|s| s.code()));
                }
            }

            if let Some(req) = stop_req {
                sup.graceful_stop(&name, &spec, &mut child, client.as_ref())
                    .await;
                {
                    let mut rt = sup.runtime.write().await;
                    if let Some(r) = rt.get_mut(&name) {
                        if r.generation == generation {
                            r.state = ServiceState::Stopped;
                            r.pid = None;
                            r.client = None;
                            r.stop_tx = None;
                            r.stopping = false;
                            r.since = Utc::now();
                        }
                    }
                }
                if let Some(req) = req {
                    let _ = req.done.send(());
                }
            } else if let Some(code) = exit_code {
                sup.handle_child_exit(&name, &spec, generation, code).await;
            }
        });
    }

    /// 优雅停止：Ipc 关 stdin 触发 EOF；Log 直接请求终止。超时兜底 kill。
    async fn graceful_stop(
        &self,
        name: &str,
        spec: &ServiceSpec,
        child: &mut Child,
        client: Option<&Arc<InitClient>>,
    ) {
        if let Some(client) = client {
            // drop BufWriter<ChildStdin> 关闭 pipe，init 端 reader EOF 后退出
            client.close_stdin().await;
        } else {
            let _ = child.start_kill();
        }
        match tokio::time::timeout(spec.stop_timeout, child.wait()).await {
            Ok(_) => debug!(service = %name, "service exited gracefully"),
            Err(_) => {
                warn!(service = %name, "service stop timed out; force killing");
                let _ = child.start_kill();
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
    }

    /// 资源监控：周期采样 pid 的 memory / cpu，超限按 `on_exceed` 处理。
    fn spawn_monitor(&self, name: String, spec: Arc<ServiceSpec>, pid: u32, generation: u64) {
        let Some(sup) = self.arc() else { return };
        let period = if spec.limits.sample_interval.is_zero() {
            Duration::from_secs(10)
        } else {
            spec.limits.sample_interval
        };
        tokio::spawn(async move {
            let mut sys = System::new();
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                {
                    let rt = sup.runtime.read().await;
                    match rt.get(&name) {
                        Some(r) if r.generation == generation && r.pid == Some(pid) => {}
                        _ => return,
                    }
                }
                let spid = Pid::from_u32(pid);
                sys.refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
                let Some(proc) = sys.process(spid) else {
                    return;
                };
                let mem_mb = proc.memory() / (1024 * 1024);
                let cpu = proc.cpu_usage();
                let mut exceeded = false;
                if let Some(max) = spec.limits.max_memory_mb {
                    if mem_mb > max {
                        exceeded = true;
                        warn!(service = %name, mem_mb = %mem_mb, max = %max, "memory limit exceeded");
                    }
                }
                if let Some(max) = spec.limits.max_cpu_percent {
                    if cpu > max {
                        exceeded = true;
                        warn!(service = %name, cpu = %cpu, max = %max, "cpu limit exceeded");
                    }
                }
                if exceeded && spec.limits.on_exceed == LimitAction::Restart {
                    warn!(service = %name, "resource limit exceeded; restarting service");
                    sup.restart_single(&name).await;
                    return;
                }
            }
        });
    }

    /// 日志文件路径。
    fn log_path(&self, spec: &ServiceSpec) -> PathBuf {
        let dir = spec
            .log_dir
            .clone()
            .unwrap_or_else(|| self.default_log_dir.clone());
        dir.join(format!("{}.log", spec.name))
    }
}

// ---------------------------------------------------------------------------
// 日志写入 + 轮转
// ---------------------------------------------------------------------------

/// 单服务日志写入器：按大小轮转，保留 `keep` 份历史。
struct LogWriter {
    main: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: Option<tokio::fs::File>,
    written: u64,
}

impl LogWriter {
    async fn open(main: PathBuf, max_bytes: u64, keep: usize) -> io::Result<Self> {
        if let Some(parent) = main.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let written = tokio::fs::metadata(&main)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&main)
            .await?;
        Ok(Self {
            main,
            max_bytes,
            keep,
            file: Some(file),
            written,
        })
    }

    async fn write_bytes(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.max_bytes > 0
            && self.written > 0
            && self.written + buf.len() as u64 > self.max_bytes
        {
            // 关闭句柄后再重命名（Windows 下打开的文件无法 rename）
            self.file = None;
            self.rotate().await?;
        }
        if let Some(f) = self.file.as_mut() {
            f.write_all(buf).await?;
            f.flush().await?;
        }
        self.written += buf.len() as u64;
        Ok(())
    }

    /// 删除最旧的 `.keep`，`.i` → `.i+1`，主文件 → `.1`，重开主文件。
    async fn rotate(&mut self) -> io::Result<()> {
        let main = self.main.clone();
        let keep = self.keep;
        let _ = tokio::fs::remove_file(rotated_path(&main, keep)).await;
        for i in (1..keep).rev() {
            let from = rotated_path(&main, i);
            if tokio::fs::metadata(&from).await.is_ok() {
                let _ = tokio::fs::rename(&from, rotated_path(&main, i + 1)).await;
            }
        }
        let _ = tokio::fs::rename(&main, rotated_path(&main, 1)).await;
        self.file = Some(
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&main)
                .await?,
        );
        self.written = 0;
        Ok(())
    }
}

/// 把 reader 的每一行追加到日志（含轮转）。
async fn pipe_to_log<R: AsyncRead + Unpin>(reader: R, writer: Arc<Mutex<LogWriter>>) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut buf = line.into_bytes();
        buf.push(b'\n');
        let mut w = writer.lock().await;
        if w.write_bytes(&buf).await.is_err() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// 单元测试（纯逻辑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, deps: &[&str]) -> Arc<ServiceSpec> {
        Arc::new(ServiceSpec {
            name: name.to_string(),
            program: PathBuf::from("prog"),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            transport: Transport::Log,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            restart: RestartPolicy::Never,
            limits: ResourceLimits::default(),
            boundary: PermissionBoundary::default(),
            log_dir: None,
            log_max_bytes: 1024,
            log_keep: 3,
            stop_timeout: Duration::from_secs(1),
        })
    }

    #[test]
    fn backoff_grows_and_caps() {
        let p = RestartPolicy::OnFailure {
            max_attempts: 10,
            backoff_ms: 1000,
            max_backoff_ms: 30000,
        };
        assert_eq!(backoff_delay(&p, 1), Some(Duration::from_millis(1000)));
        assert_eq!(backoff_delay(&p, 2), Some(Duration::from_millis(2000)));
        assert_eq!(backoff_delay(&p, 3), Some(Duration::from_millis(4000)));
        // 第 6 次为 32000ms，被 max 封顶到 30000ms
        assert_eq!(backoff_delay(&p, 6), Some(Duration::from_millis(30000)));
        assert_eq!(backoff_delay(&p, 100), Some(Duration::from_millis(30000)));
        assert_eq!(backoff_delay(&RestartPolicy::Never, 1), None);
    }

    #[test]
    fn topo_order_dependencies_first() {
        let mut m = HashMap::new();
        m.insert("c".to_string(), spec("c", &["b"]));
        m.insert("b".to_string(), spec("b", &["a"]));
        m.insert("a".to_string(), spec("a", &[]));
        assert_eq!(startup_order(&m, "c").unwrap(), vec!["a", "b", "c"]);
        assert_eq!(shutdown_order(&m, "c").unwrap(), vec!["c", "b", "a"]);
    }

    #[test]
    fn cycle_detected() {
        let mut m = HashMap::new();
        m.insert("a".to_string(), spec("a", &["b"]));
        m.insert("b".to_string(), spec("b", &["c"]));
        m.insert("c".to_string(), spec("c", &["a"]));
        assert!(startup_order(&m, "a").is_err());
    }

    #[test]
    fn self_dependency_detected() {
        let mut m = HashMap::new();
        m.insert("a".to_string(), spec("a", &["a"]));
        assert!(startup_order(&m, "a").is_err());
    }

    #[test]
    fn rotation_naming() {
        let main = Path::new("/var/log/services/init.log");
        assert_eq!(
            rotated_path(main, 0),
            PathBuf::from("/var/log/services/init.log")
        );
        assert_eq!(
            rotated_path(main, 1),
            PathBuf::from("/var/log/services/init.log.1")
        );
        assert_eq!(
            rotated_path(main, 3),
            PathBuf::from("/var/log/services/init.log.3")
        );
    }

    #[test]
    fn boundary_whitelist() {
        let b = PermissionBoundary {
            allowed_bin_roots: vec![PathBuf::from("/opt/cocktail/bin")],
            allowed_workdir_roots: vec![PathBuf::from("/srv")],
            env_denylist: Vec::new(),
        };
        // 白名单命中
        assert!(
            b.validate(Path::new("/opt/cocktail/bin/init"), None)
                .is_ok()
        );
        // 白名单未命中
        assert!(b.validate(Path::new("/opt/other/init"), None).is_err());
        assert!(
            b.validate(
                Path::new("/opt/cocktail/bin/init"),
                Some(Path::new("/srv/app"))
            )
            .is_ok()
        );
        assert!(
            b.validate(
                Path::new("/opt/cocktail/bin/init"),
                Some(Path::new("/tmp/x"))
            )
            .is_err()
        );
        // 空白名单放行
        let empty = PermissionBoundary::default();
        assert!(
            empty
                .validate(Path::new("/wherever/init"), Some(Path::new("/elsewhere")))
                .is_ok()
        );
    }
}
