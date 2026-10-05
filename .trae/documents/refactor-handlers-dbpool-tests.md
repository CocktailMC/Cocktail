# Cocktail 后端重构：handlers 拆分 + r2d2 连接池 + 全面单测

## Context

项目为 Cocktail Minecraft 控制面（Rust / axum 0.8 / rusqlite）。此前复杂度评估指出三处非必要复杂度：

1. [handlers.rs](file:///d:/Cocktail/crates/cocktail-control/src/api/handlers.rs)（2309 行、116 个 handler）是上帝模块
2. 全局 `Mutex<rusqlite::Connection>`（[state.rs](file:///d:/Cocktail/crates/cocktail-control/src/state.rs#L42)）串行化全部 DB 访问，56 处 `state.db.lock().await`
3. handlers/registry/process 三个核心模块零测试

用户已确认：连接池用 **r2d2_sqlite**；测试做**全面集成测试**。纯后端改动，前端与 API 路由不涉及任何行为变化。

---

## 改动 1：handlers.rs 按领域拆分

### 结构

`api/handlers.rs` → 目录 `api/handlers/`，内含子模块 + `mod.rs`：

```
api/handlers/
  mod.rs          # mod 声明 + pub use sub::* 重导出
  common.rs       # 共享 DTO / 辅助函数（原 handlers.rs 内定义的结构体）
  health.rs       # health
  auth.rs         # setup, login, logout, me, change_password, totp_*(4)
  settings.rs     # get_settings, update_settings
  audit.rs        # list_audit
  instances.rs    # 实例 CRUD/启停/command/eula/logs/metrics/properties/clone/preflight/
                  # version_compare/rescan/docker(3)/java(4)/fleet_summary/bulk_action/
                  # rcon(4)/events_ws/logs_ws/spec(get/apply)
  files.rs        # list/read/write/delete/download/upload/mkdir/install_jar/
                  # import_archive/set_startup_jar
  backups.rs      # list/create/delete/restore/preview
  plugin_ops.rs   # list/enable/disable + install_core + core versions/loaders + modrinth(3) + hangar(3) + spiget(4)
  players.rs      # list_players/history/action/detail/rcon_action
  worlds.rs       # list/reset/export/import/download/upload
  network.rs      # host_network + netops(5) + qq_test
  schedules.rs    # list/create/delete
  automations.rs  # list/create/delete + list_panel_events
  users.rs        # list/create/update/delete
  nodes.rs        # list/create/delete
  extensions.rs   # list/reload/set_enabled/proxy_extension_root/proxy_extension
```

### 关键规则

- `mod.rs` 中 `pub use` 全部子模块，保证 [api/mod.rs](file:///d:/Cocktail/crates/cocktail-control/src/api/mod.rs) 的 102 处 `handlers::xxx` 路由引用**零改动**
- 命名避开与 crate 顶层模块冲突：`plugins` → `plugin_ops`（crate::plugins 存在）
- 影子冲突模块（`auth`、`automations` 与 crate::auth、crate::automations 同名）：子模块内一律用全限定 `crate::auth::` / `crate::automations::` 引用
- 每个子模块自带独立 `use`（axum 提取器、`crate::instance` 类型、`crate::state::SharedState`）
- 纯剪切移动，不改任何函数体逻辑；函数体逐段搬移

---

## 改动 2：Mutex<Connection> → r2d2_sqlite 连接池

### 依赖

`crates/cocktail-control/Cargo.toml` 新增：

```toml
r2d2 = "0.8"
r2d2_sqlite = "0.25"   # 若与 rusqlite 0.32 版本不兼容，对齐到匹配版本（rusqlite 仅内部使用）
```

### db.rs

- 保留 `db::open()` 原样（CLI `run_reset_password`、`util::audit_conn` 仍用直连）
- 新增类型别名 `pub type DbPool = r2d2::Pool<SqliteConnectionManager>;`
- 新增 `pub fn pool() -> anyhow::Result<DbPool>`（默认路径）与 `pub fn pool_at(path: &Path) -> anyhow::Result<DbPool>`（测试用），两者：
  - `create_dir_all("data")`
  - `SqliteConnectionManager::file(path)` + **每连接初始化钩子**：`foreign_keys=ON`、`journal_mode=WAL`、`busy_timeout=5000`、`migrate(&conn)`
  - 初始化钩子：优先用 `SqliteConnectionManager::with_init(...)`；若无此 API，写 ~25 行 `ManageConnection` 薄包装（connect 时 open + pragmas + migrate）
  - `Pool::builder().max_size(8).build(manager)`

### state.rs

- `pub db: Mutex<rusqlite::Connection>` → `pub db: crate::db::DbPool`
- `AppState::new()`：`db::open()` → `db::pool()`；`ensure_local_node` 从池中取一个连接执行

### 56 处调用点（12 个文件）

机械替换规则：

```rust
let conn = state.db.lock().await;      // →  let conn = state.db.get().expect("db pool");
let conn = state.db.blocking_lock();   // →  let conn = state.db.get().expect("db pool");
```

- `PooledConnection` 经 Deref 到 `&rusqlite::Connection`，`db::xxx(&conn)` 全部无需改动
- 涉及文件：state.rs(7)、ops.rs(3)、netops.rs(8)、lib.rs(3)、cluster.rs(8)、instance/players.rs(3)、automations.rs(4)、api/handlers 各子模块(19)
- `util::audit_conn` 的静态直连保持不动（写审计日志，独立于池）

---

## 改动 3：全面集成测试

### 测试基建

- `db.rs` 新增 `pool_at(path)`（改动 2 已含），测试用 `tempfile::tempdir()`
- `state.rs` 新增 `#[cfg(test)] pub(crate) async fn test_state() -> SharedState`：用临时目录 pool 构建 AppState，空 instances/schedules，跳过 `hydrate_state`、env 读取、插件发现（`plugin_host`/`plugin_token` 用哑值）
- dev-dependencies 新增：`tempfile = "3"`、`http-body-util = "0.1"`、`tower`（workspace 已含 util 特性，测试用 `ServiceExt::oneshot`）

### 覆盖范围

**handlers 集成测试**（每个子模块 `#[cfg(test)] mod tests`，通过 `api::router().with_state(test_state()).oneshot(...)`）：

| 子模块 | 测试点 |
|---|---|
| health | GET /api/v1/health → 200，version/release 字段正确 |
| auth | setup 创建超管 → login 成功得 token → me → logout；密码过短 4xx；错误密码 4xx |
| instances | 空列表 200；create 校验失败 4xx（缺字段/坏 node）；create→get→update→delete 闭环 |
| users | 建角色用户→改→删（rbac 写操作） |
| automations | create→list→delete 闭环 |
| files | 不存在实例 404；write_file 到临时 workdir 后 read 回读 |
| audit | 触发一次写操作后 list_audit 有记录 |
| backups | 不存在实例/备份的 404 与校验错误路径 |

（不 spawn 真实 Minecraft 进程；start/stop 只断言错误路径。）

**registry 单测**（`instance/registry.rs`）：`ensure_port_free` 端口占用判定（空/被占/自身占用豁免）；`create_instance` 的 node 校验与端口冲突错误路径。

**process 单测**（`instance/process.rs`）：实现时读取该文件选取纯函数——JVM `-Xmx` 内存注入、`build_command` 命令构造、参数解析等，逐函数写断言。

---

## 验证

```bash
cd d:\Cocktail
cargo fmt --all -- --check        # 仓库已 rustfmt，新代码必须通过
cargo build -p cocktail-control
cargo test -p cocktail-control    # 全部新旧单测
cargo clippy -p cocktail-control --all-targets -- -D warnings   # 对齐 CI
cargo test --workspace            # 插件 crate 不受影响确认
```

前端（admin/）与打包脚本零改动。API 路由表行为不变（102 条路由原样），可人工核对 `api/mod.rs` diff 为空即可确认。

## 涉及文件

- 新增：`api/handlers/` 下 17 个文件、`Cargo.toml` dev-deps、测试文件
- 修改：`api/handlers.rs`（删除）、`api/mod.rs`（不动或仅删 `mod handlers;` 的路径）、`db.rs`、`state.rs`、`Cargo.toml`（deps）、56 处 `state.db.*` 调用点
