//! 全局事件出口：handler 侧任意位置可调 [`emit`]，把 Event 回推给 control。
//!
//! 为什么不用改 `register` 的 handler 签名：main.rs 注册了约 40 个 handler，
//! 逐个塞 writer 会造成巨量无谓改动。这里退化为模块级出口——由 [`crate::server::Server::run`]
//! 在启动时 [`install`] 一个 sink（`tokio::sync::mpsc::UnboundedSender`），
//! 由同一 runtime 上的写任务消费并通过已有的 writer 锁落帧，保证 Event 与
//! Response 不交错撕裂。
//!
//! `emit` 同步、非阻塞（`unbounded_send`）：init 是 `current_thread` runtime，
//! handler 在 async 上下文里调用，绝不能 `blocking_send`。未安装 sink 时
//! （单测 / 无 writer）静默丢弃，不 panic。

use std::sync::OnceLock;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::proto::Event;

/// 事件 sink：把 Event 投递进 [`crate::server::Server::run`] 的写任务。
pub struct EventSink {
    tx: UnboundedSender<Event>,
}

impl EventSink {
    /// 用底层 sender 构造 sink。
    pub(crate) fn new(tx: UnboundedSender<Event>) -> Self {
        Self { tx }
    }

    /// 非阻塞投递；channel 关闭（Server 已退出）时忽略错误。
    fn deliver(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

/// 全局 sink 槽位。
static SINK: OnceLock<EventSink> = OnceLock::new();

/// 创建 (sink, receiver) 通道，供 `Server::run` 起写任务。
pub(crate) fn channel() -> (EventSink, UnboundedReceiver<Event>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (EventSink::new(tx), rx)
}

/// 安装全局 sink。重复安装（如多次 `run`）静默忽略，保留首次安装者。
pub(crate) fn install(sink: EventSink) {
    let _ = SINK.set(sink);
}

/// 全局事件出口：任意 handler 可调用。
///
/// 未安装时静默丢弃（单测 / 无 writer），不 panic。
pub fn emit(method: &str, params: serde_json::Value) {
    if let Some(sink) = SINK.get() {
        sink.deliver(Event::new(method, params));
    }
}
