//! 统一日志的实时输出层：跑马灯进度行（[`LiveLine`]）+ tracing 适配（[`CocktailFormat`]）。
//!
//! 输出目标统一为 stderr。运行中行在 TTY 上用 `\r` 原地刷新并在行尾追加跑马灯，
//! 由独立 `std::thread` 驱动——绝不在 tokio runtime 上阻塞。init 进程的 stdout
//! 是 RPC 通道，日志只能走 stderr，这里正是为此设计。

use std::fmt;
use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Local;

use crate::logfmt::{self, Badge};

/// 跑马灯刷新周期。
const SPINNER_INTERVAL: Duration = Duration::from_millis(120);
/// 跑马灯条内宽与星块宽度。
const MARQUEE_WIDTH: usize = 16;
const MARQUEE_BLOCK: usize = 6;

/// 全局重绘锁：串行化多个 live 行对同一终端行的 `\r` 写入。
fn redraw_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

/// 当前持有终端行所有权的 live 行 id（0 表示无）。后开的行会接管终端行。
fn owner_slot() -> &'static Mutex<u64> {
    static O: OnceLock<Mutex<u64>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(0))
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// 是否 TTY（stderr）。
pub fn is_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// 直接打印一行（非实时），输出到 stderr。
///
/// 与跑马灯线程共用重绘锁：TTY 下先回车清除当前实时行，避免新日志与
/// 跑马灯行在同一终端行上互相覆盖、错位。
pub fn emit(badge: Badge, source: &str, event: &str, kv: Vec<(String, String)>) {
    let line = logfmt::render_line(Local::now(), badge, source, event, &kv);
    let _guard = redraw_lock().lock().unwrap();
    let mut err = std::io::stderr().lock();
    if is_tty() {
        let _ = write!(err, "\r{line}\n");
    } else {
        let _ = writeln!(err, "{line}");
    }
    let _ = err.flush();
}

/// 实时进度行句柄：TTY 上原地刷新（`\r` + 跑马灯），非 TTY 只打印静态行。
///
/// 内部跑马灯线程通过 `Drop` 保证 join，避免残留 `\r` 行。`LiveLine` 是 `Send`，
/// 可在 `tokio::spawn` 的 future 里创建并跨 `await` 持有。
pub struct LiveLine {
    id: u64,
    source: String,
    event: String,
    kv: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    tty: bool,
}

impl LiveLine {
    /// 开一行运行中（[`Badge::Running`]）。
    pub fn begin(
        source: impl Into<String>,
        event: impl Into<String>,
        kv: Vec<(String, String)>,
    ) -> Self {
        let source = source.into();
        let event = event.into();
        let kv = Arc::new(Mutex::new(kv));
        let stop = Arc::new(AtomicBool::new(false));
        let tty = is_tty();
        let id = next_id();

        let handle = if tty {
            // 接管终端行
            *owner_slot().lock().unwrap() = id;
            let source_c = source.clone();
            let event_c = event.clone();
            let kv_c = Arc::clone(&kv);
            let stop_c = Arc::clone(&stop);
            Some(std::thread::spawn(move || {
                let mut tick = 0u64;
                loop {
                    if stop_c.load(Ordering::SeqCst) {
                        break;
                    }
                    // 失去所有权后停止重绘，避免与更晚的 live 行互踩
                    if *owner_slot().lock().unwrap() != id {
                        break;
                    }
                    let kv = kv_c.lock().unwrap().clone();
                    let base =
                        logfmt::render_line(Local::now(), Badge::Running, &source_c, &event_c, &kv);
                    let line = format!(
                        "{base} {}",
                        logfmt::marquee(tick, MARQUEE_WIDTH, MARQUEE_BLOCK)
                    );
                    {
                        let _guard = redraw_lock().lock().unwrap();
                        if *owner_slot().lock().unwrap() != id {
                            break;
                        }
                        let mut err = std::io::stderr().lock();
                        let _ = write!(err, "\r{line}");
                        let _ = err.flush();
                    }
                    tick = tick.wrapping_add(1);
                    std::thread::sleep(SPINNER_INTERVAL);
                }
            }))
        } else {
            // 非 TTY：只打一行静态 Running
            emit(Badge::Running, &source, &event, kv.lock().unwrap().clone());
            None
        };

        Self {
            id,
            source,
            event,
            kv,
            stop,
            handle,
            tty,
        }
    }

    /// 更新 kv（如 received/total），原地重绘。
    pub fn update(&self, kv: Vec<(String, String)>) {
        *self.kv.lock().unwrap() = kv;
    }

    /// 收尾为 DONE。
    pub fn done(self, event: impl Into<String>, kv: Vec<(String, String)>) {
        self.finish(Badge::Done, event.into(), kv);
    }

    /// 收尾为 WARN。
    pub fn warn(self, event: impl Into<String>, kv: Vec<(String, String)>) {
        self.finish(Badge::Warn, event.into(), kv);
    }

    /// 收尾为 FAIL；`error` 值一律加双引号，并带上此前的最新 kv。
    pub fn fail(self, event: impl Into<String>, error: &str) {
        let mut kv = self.kv.lock().unwrap().clone();
        kv.push(("error".to_string(), format!("\"{error}\"")));
        self.finish(Badge::Fail, event.into(), kv);
    }

    /// 是否仍持有终端行所有权。
    fn owns(&self) -> bool {
        self.tty && *owner_slot().lock().unwrap() == self.id
    }

    /// 释放终端行所有权（仅当自己仍是 owner 时）。
    fn release(&self) {
        if !self.tty {
            return;
        }
        let mut o = owner_slot().lock().unwrap();
        if *o == self.id {
            *o = 0;
        }
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    /// 收尾：停线程后，若仍持有终端行则用最终行覆盖跑马灯行，否则退化为普通新行。
    fn finish(mut self, badge: Badge, event: String, kv: Vec<(String, String)>) {
        self.stop_and_join();
        let owns = self.owns();
        let line = logfmt::render_line(Local::now(), badge, &self.source, &event, &kv);
        {
            let _guard = redraw_lock().lock().unwrap();
            let mut err = std::io::stderr().lock();
            if owns {
                let _ = write!(err, "\r{line}\n");
            } else {
                let _ = writeln!(err, "{line}");
            }
            let _ = err.flush();
        }
        self.release();
        // self 在此 drop：handle 已为 None，release 已完成，Drop 不会再动终端。
    }
}

impl Drop for LiveLine {
    fn drop(&mut self) {
        let had_handle = self.handle.is_some();
        self.stop_and_join();
        if !self.tty {
            return;
        }
        // 未显式收尾（如 `?` 提前返回）时，终结跑马灯行，避免残留 `\r`。
        if *owner_slot().lock().unwrap() == self.id {
            let _guard = redraw_lock().lock().unwrap();
            let mut err = std::io::stderr().lock();
            if had_handle {
                let _ = writeln!(err);
            }
            let _ = err.flush();
        }
        let mut o = owner_slot().lock().unwrap();
        if *o == self.id {
            *o = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// tracing 适配
// ---------------------------------------------------------------------------

/// 把 tracing 事件渲染成统一格式：`[ts][ BADG ][source] event  k=v`。
///
/// 级别映射：ERROR→FAIL、WARN→WARN、INFO→INFO、DEBUG→DBUG、TRACE→TRCE。
/// 组件名用 `event.metadata().target()` 过 [`logfmt::source_name`]；message 字段作为
/// 事件名，其余字段作为 kv。
pub struct CocktailFormat;

#[derive(Default)]
struct FieldCollector {
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for FieldCollector {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.fields
            .push((field.name().to_string(), format!("{value:?}")));
    }
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for CocktailFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let badge = match *meta.level() {
            tracing::Level::ERROR => Badge::Fail,
            tracing::Level::WARN => Badge::Warn,
            tracing::Level::INFO => Badge::Info,
            tracing::Level::DEBUG => Badge::Debug,
            tracing::Level::TRACE => Badge::Trace,
        };
        let source = logfmt::source_name(meta.target());

        let mut collector = FieldCollector::default();
        event.record(&mut collector);
        let mut event_name = String::new();
        let mut kv = Vec::new();
        for (name, value) in collector.fields {
            if name == "message" {
                event_name = value;
            } else {
                kv.push((name, value));
            }
        }

        let line = logfmt::render_line(Local::now(), badge, &source, &event_name, &kv);
        writeln!(writer, "{line}")
    }
}
