//! Cocktail 共享类型：control plane 与 init 子进程之间 IPC 消息使用。
//!
//! 设计原则：只放纯数据结构（serde 友好），不持有句柄、不依赖 tokio。
//! `Instance` 等带运行时句柄的类型由 control/init 各自维护本地版本，
//! 通过 `InstanceView`/`InstanceSpec` 等 IPC 友好的纯数据类型跨进程传递。
//!
//! 阶段 2 进行中：内容待 sub-agent 从 control/src/instance/model.rs
//! 与 control/src/proto.rs 抽取填充。

pub mod java;
pub mod logfmt;
pub mod logging;
pub mod model;
pub mod proto;
pub mod sevenz;
pub mod versions;

pub mod runtime;
pub mod runtime_util;
pub mod stdin_bridge;
pub mod wincompat;
pub mod winnet;
