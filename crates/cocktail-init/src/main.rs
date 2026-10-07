//! cocktail-init 入口：从 stdin 读 JSON-RPC 帧，dispatch，写 Response 到 stdout。
//!
//! 日志走 stderr（control 端转发到自己的 tracing）。
//! control 退出时关闭 stdin，本进程读到 EOF 自然退出。

use cocktail_init::server::Server;
use cocktail_init::{archive, files, http, java, proto, rcon, secrets, sevenz, versions};

use serde::Deserialize;
use std::path::Path;

/// sevenz.extract RPC 参数。
#[derive(Debug, Deserialize)]
struct ExtractParams {
    archive: String,
    dest: String,
}

/// sevenz.is_supported_name RPC 参数。
#[derive(Debug, Deserialize)]
struct NameParams {
    name: String,
}

/// http.download_to_path RPC 参数。
#[derive(Debug, Deserialize)]
struct DownloadParams {
    url: String,
    dest: String,
}

/// 通用参数：workdir + relative（list_files/read_file/write_file 等共用）。
#[derive(Debug, Deserialize)]
struct WorkdirRelativeParams {
    workdir: String,
    relative: String,
}

/// 通用参数：仅 instance_id（list_backups 等共用）。
#[derive(Debug, Deserialize)]
struct InstanceIdParams {
    instance_id: String,
}

/// write_file 参数。
#[derive(Debug, Deserialize)]
struct WriteFileParams {
    workdir: String,
    relative: String,
    content: String,
}

/// write_bytes / extract_zip_into 参数（字节走 Vec<u8>）。
#[derive(Debug, Deserialize)]
struct WriteBytesParams {
    workdir: String,
    relative: String,
    bytes: Vec<u8>,
}

/// ensure_seed_files 参数。
#[derive(Debug, Deserialize)]
struct EnsureSeedParams {
    workdir: String,
    port: u16,
    eula_accepted: bool,
}

/// sync_port 参数。
#[derive(Debug, Deserialize)]
struct SyncPortParams {
    workdir: String,
    port: u16,
}

/// create_backup 参数。
#[derive(Debug, Deserialize)]
struct CreateBackupParams {
    instance_id: String,
    workdir: String,
}

/// prune_backups 参数。
#[derive(Debug, Deserialize)]
struct PruneBackupsParams {
    instance_id: String,
    keep: u32,
}

/// backup_path / backup_meta 参数。
#[derive(Debug, Deserialize)]
struct BackupRefParams {
    instance_id: String,
    backup_id: String,
}

/// restore_backup 参数。
#[derive(Debug, Deserialize)]
struct RestoreBackupParams {
    instance_id: String,
    backup_id: String,
    workdir: String,
}

/// inspect_backup_zip 参数。
#[derive(Debug, Deserialize)]
struct InspectBackupZipParams {
    path: String,
}

/// unzip_archive 参数。
#[derive(Debug, Deserialize)]
struct UnzipArchiveParams {
    zip_path: String,
    dest: String,
}

/// copy_instance_tree 参数。
#[derive(Debug, Deserialize)]
struct CopyInstanceTreeParams {
    src: String,
    dst: String,
    copy_data: bool,
    skip_logs: bool,
}

/// pack_subdir_zip 参数。
#[derive(Debug, Deserialize)]
struct PackSubdirZipParams {
    workdir: String,
    relative: String,
    dest: String,
}

/// default_instance_root 参数。
#[derive(Debug, Deserialize)]
struct DefaultInstanceRootParams {
    id: String,
}

/// workdirs_conflict 参数。
#[derive(Debug, Deserialize)]
struct WorkdirsConflictParams {
    a: String,
    b: String,
}

/// total_dir_bytes 参数。
#[derive(Debug, Deserialize)]
struct TotalDirBytesParams {
    path: String,
}

/// write_mc_version_marker 参数。
#[derive(Debug, Deserialize)]
struct WriteMcVersionMarkerParams {
    workdir: String,
    version: String,
}

/// java.install RPC 参数：major + image_type（"jre"|"jdk"）。
#[derive(Debug, Deserialize)]
struct JavaInstallParams {
    major: u32,
    #[serde(default)]
    image_type: Option<String>,
}

/// java.remove RPC 参数：runtime id（如 "temurin-21-jre"）。
#[derive(Debug, Deserialize)]
struct JavaIdParams {
    id: String,
}

/// java.ensure_for_spec / java.ensure_instance_jre 参数。
#[derive(Debug, Deserialize)]
struct JavaEnsureForSpecParams {
    workdir: String,
    #[serde(default)]
    java_major: Option<u32>,
    #[serde(default)]
    mc_version: Option<String>,
}

/// java.find_managed 参数。
#[derive(Debug, Deserialize)]
struct JavaFindManagedParams {
    major: u32,
    #[serde(default)]
    prefer: Option<String>,
}

/// versions.list_versions 参数。
#[derive(Debug, Deserialize)]
struct VersionsCoreParams {
    core: String,
}

/// versions.list_loaders 参数。
#[derive(Debug, Deserialize)]
struct VersionsCoreVerParams {
    core: String,
    version: String,
}

/// versions.download_and_install 参数。
#[derive(Debug, Deserialize)]
struct VersionsInstallParams {
    workdir: String,
    core: String,
    version: String,
    #[serde(default)]
    loader: Option<String>,
}

/// archive.extract_pack 参数。
#[derive(Debug, Deserialize)]
struct ArchiveExtractPackParams {
    archive_path: String,
    workdir: String,
    filename: String,
}

/// archive.resolve_startup 参数。
#[derive(Debug, Deserialize)]
struct ArchiveResolveStartupParams {
    workdir: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    // 日志走 stderr（control 端转发到自己的 tracing）。
    // 阶段 1 用最简初始化：RUST_LOG=info 时输出到 stderr。
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    tracing::info!("cocktail-init starting (pid={})", std::process::id());

    let mut server = Server::new();

    // secrets.get_master_key：返回 32 字节密钥（hex 字符串形式，避免 JSON 字节数组麻烦）
    server.register("secrets.get_master_key", |_params| async move {
        match secrets::get_master_key() {
            Ok(key) => Ok(serde_json::json!({
                "key_hex": hex_encode(&key),
                "source": secrets::key_source(),
            })),
            Err(msg) => Err(proto::Error::internal(msg)),
        }
    });

    // secrets.key_source：返回密钥来源描述
    server.register("secrets.key_source", |_params| async move {
        Ok(serde_json::json!({ "source": secrets::key_source() }))
    });

    // sevenz.ensure_bin：落地内置 7z 并返回路径。失败返 RPC error。
    server.register("sevenz.ensure_bin", |_params| async move {
        match sevenz::ensure_bin() {
            Ok(path) => {
                let embedded = sevenz::EMBEDDED.is_some();
                Ok(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "embedded": embedded,
                }))
            }
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // sevenz.extract：解压 archive 到 dest。params: { archive, dest }
    server.register("sevenz.extract", |params| async move {
        let p: ExtractParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match sevenz::extract(
            std::path::Path::new(&p.archive),
            std::path::Path::new(&p.dest),
        ) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // sevenz.is_supported_name：纯字符串校验，params: { name }
    server.register("sevenz.is_supported_name", |params| async move {
        let p: NameParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "supported": sevenz::is_supported_name(&p.name) }))
    });

    // rcon.try_rcon：TCP 连接测试
    server.register("rcon.try_rcon", |params| async move {
        let params: rcon::TryRconParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        let r = rcon::try_rcon(params).await;
        Ok(serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
    });

    // http.download_to_path：下载 url 到 dest（init 端构建 client，含代理检测）。
    // 进度推送留到阶段 2 event push 通道就位后再实现。
    server.register("http.download_to_path", |params| async move {
        let p: DownloadParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match http::download_to_path(&p.url, std::path::Path::new(&p.dest)).await {
            Ok(n) => Ok(serde_json::json!({ "written": n })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.resolve_in_workdir：在 workdir 内解析相对路径，做 zip-slip 防护。
    server.register("files.resolve_in_workdir", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::resolve_in_workdir(&p.workdir, &p.relative) {
            Ok(path) => Ok(serde_json::json!({ "path": path.to_string_lossy() })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.list_files：列出 workdir/relative 目录下的条目。
    server.register("files.list_files", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::list_files(&p.workdir, &p.relative) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.read_file：读取文本文件（≤2MiB）。
    server.register("files.read_file", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::read_file(&p.workdir, &p.relative) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.read_bytes：读取二进制文件（≤512MiB）。返回 { path, bytes }。
    server.register("files.read_bytes", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::read_bytes(&p.workdir, &p.relative) {
            Ok((path, bytes)) => Ok(serde_json::json!({ "path": path, "bytes": bytes })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.write_file：写入文本文件。
    server.register("files.write_file", |params| async move {
        let p: WriteFileParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::write_file(&p.workdir, &p.relative, &p.content) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.write_bytes：写入二进制文件。
    server.register("files.write_bytes", |params| async move {
        let p: WriteBytesParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::write_bytes(&p.workdir, &p.relative, &p.bytes) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.delete_path：删除 workdir 内的相对路径。
    server.register("files.delete_path", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::delete_path(&p.workdir, &p.relative) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.mkdir：在 workdir 内创建目录。
    server.register("files.mkdir", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::mkdir(&p.workdir, &p.relative) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.jar_exists：检查 workdir/relative 是否为已存在的 jar 文件。
    server.register("files.jar_exists", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "exists": files::jar_exists(&p.workdir, &p.relative) }))
    });

    // files.default_instance_root：算出某实例 id 的默认 data/instances/<id> 路径。
    server.register("files.default_instance_root", |params| async move {
        let p: DefaultInstanceRootParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "root": files::default_instance_root(&p.id) }))
    });

    // files.workdirs_conflict：判断两个 workdir 是否重叠（component-aware）。
    server.register("files.workdirs_conflict", |params| async move {
        let p: WorkdirsConflictParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "conflict": files::workdirs_conflict(&p.a, &p.b) }))
    });

    // files.ensure_seed_files：建好实例种子目录结构与 server.properties/eula.txt。
    server.register("files.ensure_seed_files", |params| async move {
        let p: EnsureSeedParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::ensure_seed_files(&p.workdir, p.port, p.eula_accepted) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.sync_port：把 server-port= 同步进 server.properties。
    server.register("files.sync_port", |params| async move {
        let p: SyncPortParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::sync_port(&p.workdir, p.port) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.create_backup：把 workdir 打包成 data/backups/<id>/<stamp>.zip。
    server.register("files.create_backup", |params| async move {
        let p: CreateBackupParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::create_backup(&p.instance_id, &p.workdir) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.prune_backups：保留最近 keep 个备份，删其余。
    server.register("files.prune_backups", |params| async move {
        let p: PruneBackupsParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::prune_backups(&p.instance_id, p.keep) {
            Ok(n) => Ok(serde_json::json!({ "pruned": n })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.backup_path：返回某备份的绝对 PathBuf（带 file_name 防穿越）。
    server.register("files.backup_path", |params| async move {
        let p: BackupRefParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::backup_path(&p.instance_id, &p.backup_id) {
            Ok(path) => Ok(serde_json::json!({ "path": path.to_string_lossy() })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.backup_meta：返回某备份的 BackupInfo（id/created_at/path/size）。
    server.register("files.backup_meta", |params| async move {
        let p: BackupRefParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::backup_meta(&p.instance_id, &p.backup_id) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.inspect_backup_zip：扫描备份内容（zip 或目录），返回 BackupScan。
    server.register("files.inspect_backup_zip", |params| async move {
        let p: InspectBackupZipParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::inspect_backup_zip(Path::new(&p.path)) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.list_backups：列出某实例的所有备份。
    server.register("files.list_backups", |params| async move {
        let p: InstanceIdParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::list_backups(&p.instance_id) {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.delete_backup：删除某实例的指定备份。
    server.register("files.delete_backup", |params| async move {
        let p: BackupRefParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::delete_backup(&p.instance_id, &p.backup_id) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.restore_backup：把备份内容解压/复制回 workdir。
    server.register("files.restore_backup", |params| async move {
        let p: RestoreBackupParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::restore_backup(&p.instance_id, &p.backup_id, &p.workdir) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.unzip_archive：通用解压 zip 到 dest。
    server.register("files.unzip_archive", |params| async move {
        let p: UnzipArchiveParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::unzip_archive(Path::new(&p.zip_path), Path::new(&p.dest)) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.copy_instance_tree：复制实例目录树（带 skip_logs/copy_data 选项）。
    server.register("files.copy_instance_tree", |params| async move {
        let p: CopyInstanceTreeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::copy_instance_tree(&p.src, &p.dst, p.copy_data, p.skip_logs) {
            Ok(n) => Ok(serde_json::json!({ "copied": n })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.pack_subdir_zip：把 workdir/relative 子目录打包成 dest zip。
    server.register("files.pack_subdir_zip", |params| async move {
        let p: PackSubdirZipParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::pack_subdir_zip(&p.workdir, &p.relative, Path::new(&p.dest)) {
            Ok(n) => Ok(serde_json::json!({ "size": n })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.extract_zip_into：把 zip 字节流解压合并到 workdir/relative 下。
    server.register("files.extract_zip_into", |params| async move {
        let p: WriteBytesParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::extract_zip_into(&p.workdir, &p.relative, &p.bytes) {
            Ok(n) => Ok(serde_json::json!({ "files": n })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.guess_mc_version：根据 properties/jars/logs 推断 MC 版本。
    server.register("files.guess_mc_version", |params| async move {
        let p: WorkdirRelativeParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "version": files::guess_mc_version(&p.workdir) }))
    });

    // files.write_mc_version_marker：把 cocktail-mc-version= 写进 server.properties。
    server.register("files.write_mc_version_marker", |params| async move {
        let p: WriteMcVersionMarkerParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match files::write_mc_version_marker(&p.workdir, &p.version) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // files.total_dir_bytes：递归统计某目录占用字节数。
    server.register("files.total_dir_bytes", |params| async move {
        let p: TotalDirBytesParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "bytes": files::total_dir_bytes(&p.path) }))
    });

    // java.inventory：盘点系统 + 已安装的 JRE，无参数。
    server.register("java.inventory", |_params| async move {
        let v = java::inventory().await;
        Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null))
    });

    // java.install：按主版本号下载并安装一份 Temurin JRE/JDK。params: { major, image_type }
    server.register("java.install", |params| async move {
        let p: JavaInstallParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        let image =
            match cocktail_shared::java::ImageType::parse(p.image_type.as_deref().unwrap_or("jre"))
            {
                Ok(v) => v,
                Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
            };
        match java::install(p.major, image).await {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // java.remove：按 id 删除已安装的运行时。params: { id }
    server.register("java.remove", |params| async move {
        let p: JavaIdParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match java::remove(&p.id) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // java.ensure_api：按 EnsureJavaRequest 选合适的 JRE，找不到就装。
    server.register("java.ensure_api", |params| async move {
        let req: cocktail_shared::java::EnsureJavaRequest = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match java::ensure_api(req).await {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // java.ensure_for_spec：按 workdir + java_major + mc_version 确保实例有可用 JRE。
    // 返回 { java_bin: String }（PathBuf.to_string_lossy）。
    server.register("java.ensure_for_spec", |params| async move {
        let p: JavaEnsureForSpecParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match java::ensure_for_spec(&p.workdir, p.java_major, p.mc_version.as_deref()).await {
            Ok(bin) => Ok(serde_json::json!({ "java_bin": bin.to_string_lossy() })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // java.ensure_instance_jre：与 ensure_for_spec 同参同返（保留独立 RPC 便于直调）。
    server.register("java.ensure_instance_jre", |params| async move {
        let p: JavaEnsureForSpecParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match java::ensure_instance_jre(&p.workdir, p.java_major, p.mc_version.as_deref()).await {
            Ok(bin) => Ok(serde_json::json!({ "java_bin": bin.to_string_lossy() })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // java.probe_system：探测 PATH 上的 java，无参数，返 Option<SystemJava>。
    server.register("java.probe_system", |_params| async move {
        match java::probe_system().await {
            Some(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            None => Ok(serde_json::Value::Null),
        }
    });

    // java.list_installed：列出 data/java 下所有已安装运行时，无参数。
    server.register("java.list_installed", |_params| async move {
        let v = java::list_installed();
        Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null))
    });

    // java.find_managed：按 major + prefer 找已安装运行时。返 Option<InstalledRuntime>。
    server.register("java.find_managed", |params| async move {
        let p: JavaFindManagedParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        let prefer = match p
            .prefer
            .as_deref()
            .map(cocktail_shared::java::ImageType::parse)
            .transpose()
        {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match java::find_managed(p.major, prefer) {
            Some(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            None => Ok(serde_json::Value::Null),
        }
    });

    // versions.list_versions：列出某核心的可用 MC 版本。params: { core }
    // 返回 Vec<CoreVersion>。
    server.register("versions.list_versions", |params| async move {
        let p: VersionsCoreParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match versions::list_versions(&p.core).await {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // versions.list_loaders：列出某 MC 版本下可选 loader。params: { core, version }
    // 返回 Vec<CoreLoader>。
    server.register("versions.list_loaders", |params| async move {
        let p: VersionsCoreVerParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match versions::list_loaders(&p.core, &p.version).await {
            Ok(v) => Ok(serde_json::to_value(&v).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // versions.download_and_install：下载并落地服务端 jar / 跑 modloader 安装器。
    // params: { workdir, core, version, loader? } 返回 { command, args }。
    server.register("versions.download_and_install", |params| async move {
        let p: VersionsInstallParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match versions::download_and_install(&p.workdir, &p.core, &p.version, p.loader.as_deref())
            .await
        {
            Ok((command, args)) => Ok(serde_json::json!({ "command": command, "args": args })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // archive.extract_pack：解压 → 去嵌套 → 去 junk → 扁平化 → 越界防护 → 合并到 workdir。
    // params: { archive_path, workdir, filename } 返回 { flattened, files }。
    // TODO 阶段 2：原 control 端 `http::Transfer` 进度推送（"extract" phase）暂砍，
    // 待 event push 通道就位后用 init→control 单向事件回传 progress。
    server.register("archive.extract_pack", |params| async move {
        let p: ArchiveExtractPackParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match archive::extract_pack(
            Path::new(&p.archive_path),
            Path::new(&p.workdir),
            &p.filename,
        )
        .await
        {
            Ok(out) => Ok(serde_json::to_value(&out).unwrap_or(serde_json::Value::Null)),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // archive.resolve_startup：解析启动命令。用户 override 优先，否则扫描 workdir。
    // params: { workdir, command?, args? } 返回 { startup, command, args }。
    server.register("archive.resolve_startup", |params| async move {
        let p: ArchiveResolveStartupParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match archive::resolve_startup(&p.workdir, p.command.as_deref(), &p.args) {
            Ok((startup, command, args)) => Ok(serde_json::json!({
                "startup": startup,
                "command": command,
                "args": args,
            })),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // 用 stdin/stdout 跑主循环
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    server.run(stdin, stdout).await
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
