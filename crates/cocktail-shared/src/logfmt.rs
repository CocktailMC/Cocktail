//! 统一控制台日志格式（纯逻辑，零 IO，可单元测试）。
//!
//! 目标格式：
//! ```text
//! [2026/10/07 12:07:30][ WAIT ][cocktail-init ] init.waiting  parent=control
//! ```
//! 组成：`[本地时间][ 4 字符徽章 ][组件名] 事件名  key=value  key=value`。
//! 运行中行在行尾追加跑马灯 `[    ******        ]`（见 [`marquee`]）。
//!
//! control 与 init 两个进程共用本模块，保证终端输出完全一致。

use std::time::Duration;

/// 行徽章：4 字符定宽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Badge {
    Wait,
    Running,
    Done,
    Warn,
    Fail,
    Info,
    Debug,
    Trace,
}

impl Badge {
    /// 徽章文本（恒为 4 字符）。
    pub fn text(self) -> &'static str {
        match self {
            Badge::Wait => "WAIT",
            Badge::Running => "****",
            Badge::Done => "DONE",
            Badge::Warn => "WARN",
            Badge::Fail => "FAIL",
            Badge::Info => "INFO",
            Badge::Debug => "DBUG",
            Badge::Trace => "TRCE",
        }
    }
}

/// 组件名固定宽度：这是唯一需要调的空格量。
/// 渲染为 `[` + 名称左对齐补齐到本宽度 + `]`（长于该宽度时自然溢出、不截断）。
pub const SOURCE_WIDTH: usize = 14;

/// 本地时间戳，格式 `%Y/%m/%d %H:%M:%S`。
pub fn format_ts(ts: chrono::DateTime<chrono::Local>) -> String {
    ts.format("%Y/%m/%d %H:%M:%S").to_string()
}

/// 组件名左对齐补齐到 [`SOURCE_WIDTH`]（幂等：已补齐的再补齐不会变长）。
fn pad_source(source: &str) -> String {
    let n = source.chars().count();
    if n >= SOURCE_WIDTH {
        return source.to_string();
    }
    let mut s = String::with_capacity(SOURCE_WIDTH);
    s.push_str(source);
    for _ in n..SOURCE_WIDTH {
        s.push(' ');
    }
    s
}

/// kv 值渲染：含空白则加双引号；已是双引号包裹的原样返回（幂等）。
pub fn render_value(v: &str) -> String {
    if v.chars().count() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return v.to_string();
    }
    if v.chars().any(char::is_whitespace) {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// 渲染完整一行（不含行尾换行）：`[ts][ BADG ][source] event  k=v  k=v`。
pub fn render_line(
    ts: chrono::DateTime<chrono::Local>,
    badge: Badge,
    source: &str,
    event: &str,
    kv: &[(String, String)],
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = write!(
        s,
        "[{}][ {} ][{}] {}",
        format_ts(ts),
        badge.text(),
        pad_source(source),
        event
    );
    for (k, v) in kv {
        let _ = write!(s, "  {k}={}", render_value(v));
    }
    s
}

/// 跑马灯条：内宽 `width`、星块 `block`，`tick` 递增使星块从左向右滚动并循环。
/// 返回形如 `[    ******        ]` 的字符串，长度恒为 `width + 2`。
pub fn marquee(tick: u64, width: usize, block: usize) -> String {
    let width = width.max(1);
    let block = block.clamp(1, width);
    // 起始位置数：0 ..= width - block
    let span = width - block + 1;
    let offset = (tick % span as u64) as usize;
    let mut s = String::with_capacity(width + 2);
    s.push('[');
    for i in 0..width {
        s.push(if i >= offset && i < offset + block {
            '*'
        } else {
            ' '
        });
    }
    s.push(']');
    s
}

/// bytes → 人类可读体积，如 `122.4MiB`。
pub fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GiB", b / GIB)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}MiB", b / MIB)
    } else if bytes >= 1024 {
        format!("{:.1}KiB", b / KIB)
    } else {
        format!("{bytes}B")
    }
}

/// Duration → `4.2s` / `126ms`（小于 1s 用毫秒，否则保留一位小数秒）。
pub fn human_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

/// tracing target → 短组件名，并左对齐补齐到 [`SOURCE_WIDTH`]。
///
/// - `cocktail_control` → `cocktail-control`
/// - `cocktail_control::http` → `cocktail-http`
/// - 多段 target 回落取最后一段（项目前缀取首段首个 `_` 前的词）。
pub fn source_name(target: &str) -> String {
    let mut parts = target.split("::");
    let first = parts.next().unwrap_or(target);
    let rest: Vec<&str> = parts.collect();
    let name = if rest.is_empty() {
        first.replace('_', "-")
    } else {
        let leaf = rest.last().copied().unwrap_or(first);
        let root = first.split('_').next().unwrap_or(first);
        format!("{root}-{leaf}")
    };
    pad_source(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn ts() -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 7, 12, 7, 30).unwrap()
    }

    #[test]
    fn badge_text_is_four_chars() {
        for b in [
            Badge::Wait,
            Badge::Running,
            Badge::Done,
            Badge::Warn,
            Badge::Fail,
            Badge::Info,
            Badge::Debug,
            Badge::Trace,
        ] {
            assert_eq!(b.text().chars().count(), 4);
        }
        assert_eq!(Badge::Running.text(), "****");
        assert_eq!(Badge::Done.text(), "DONE");
        assert_eq!(Badge::Debug.text(), "DBUG");
        assert_eq!(Badge::Trace.text(), "TRCE");
    }

    #[test]
    fn human_bytes_boundaries() {
        assert_eq!(human_bytes(0), "0B");
        assert_eq!(human_bytes(512), "512B");
        assert_eq!(human_bytes(1023), "1023B");
        assert_eq!(human_bytes(1024), "1.0KiB");
        assert_eq!(human_bytes(2048), "2.0KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0MiB");
        assert_eq!(human_bytes(128_345_702), "122.4MiB");
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.0GiB");
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.0GiB");
    }

    #[test]
    fn human_duration_branches() {
        assert_eq!(human_duration(Duration::from_millis(126)), "126ms");
        assert_eq!(human_duration(Duration::from_millis(999)), "999ms");
        assert_eq!(human_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(human_duration(Duration::from_millis(4200)), "4.2s");
    }

    #[test]
    fn render_value_quotes_whitespace() {
        assert_eq!(render_value("abc"), "abc");
        assert_eq!(render_value(""), "");
        assert_eq!(render_value("a b"), "\"a b\"");
        assert_eq!(render_value("access denied"), "\"access denied\"");
        // 已引号包裹时幂等
        assert_eq!(render_value("\"a b\""), "\"a b\"");
    }

    #[test]
    fn marquee_scrolls_and_wraps() {
        let (w, b) = (16usize, 6usize);
        let m0 = marquee(0, w, b);
        assert_eq!(m0.chars().count(), w + 2);
        assert_eq!(m0, "[******          ]");
        // 不同 tick 偏移不同
        assert_ne!(marquee(3, w, b), marquee(4, w, b));
        // 循环回绕：span = 16 - 6 + 1 = 11
        assert_eq!(marquee(11, w, b), m0);
        assert_eq!(marquee(22, w, b), m0);
        // 长度恒定
        for t in 0..40 {
            assert_eq!(marquee(t, w, b).chars().count(), w + 2);
        }
    }

    #[test]
    fn render_line_layout() {
        let line = render_line(
            ts(),
            Badge::Wait,
            "cocktail-init",
            "init.waiting",
            &[("parent".to_string(), "control".to_string())],
        );
        assert_eq!(
            line,
            "[2026/10/07 12:07:30][ WAIT ][cocktail-init ] init.waiting  parent=control"
        );

        let line = render_line(
            ts(),
            Badge::Fail,
            "cocktail-runtime",
            "runtime.spawn_failed",
            &[
                ("instance".to_string(), "survival".to_string()),
                ("error".to_string(), "access denied".to_string()),
            ],
        );
        assert_eq!(
            line,
            "[2026/10/07 12:07:30][ FAIL ][cocktail-runtime] runtime.spawn_failed  \
             instance=survival  error=\"access denied\""
        );
    }

    #[test]
    fn source_name_mapping() {
        // 单段：`_` → `-`（16 字符，超宽不补齐）
        assert_eq!(source_name("cocktail_control"), "cocktail-control");
        // 多段：项目前缀 + 最后一段，补齐到 SOURCE_WIDTH
        assert_eq!(source_name("cocktail_control::http"), "cocktail-http ");
        assert_eq!(source_name("cocktail_init::http"), "cocktail-http ");
        assert_eq!(source_name("cocktail_control::foo::bar"), "cocktail-bar  ");
        // 全部结果长度 >= SOURCE_WIDTH
        for t in [
            "cocktail_control",
            "cocktail_control::http",
            "cocktail_init::java",
        ] {
            assert!(source_name(t).chars().count() >= SOURCE_WIDTH);
        }
    }
}
