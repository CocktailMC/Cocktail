use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

use crate::crypto;

pub const CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntryMeta {
    pub path: String,
    pub size: u64,
    pub chunks: Vec<String>,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub instance_id: String,
    pub created_at: DateTime<Utc>,
    pub label: String,
    pub files: Vec<FileEntryMeta>,
    pub total_bytes: u64,
    pub stored_bytes: u64,
    pub chunk_count: usize,
    pub parent: Option<String>,
    pub chain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreStats {
    pub objects: usize,
    pub object_bytes: u64,
    pub snapshots: usize,
    pub logical_bytes: u64,
}

pub fn backup_root(instance_id: &str) -> PathBuf {
    PathBuf::from("data")
        .join("backups")
        .join(instance_id)
        .join("vault")
}

fn objects_dir(instance_id: &str) -> PathBuf {
    backup_root(instance_id).join("objects")
}

fn snapshots_dir(instance_id: &str) -> PathBuf {
    backup_root(instance_id).join("snapshots")
}

fn object_path(instance_id: &str, hash: &str) -> PathBuf {
    objects_dir(instance_id)
        .join(&hash[..2])
        .join(hash)
}

fn snapshot_path(instance_id: &str, id: &str) -> PathBuf {
    snapshots_dir(instance_id).join(format!("{id}.json"))
}

pub fn chunk_hash(data: &[u8]) -> String {
    crypto::sha256_hex(data)
}

pub fn store_object(instance_id: &str, data: &[u8]) -> anyhow::Result<(String, bool)> {
    let hash = chunk_hash(data);
    let path = object_path(instance_id, &hash);
    if path.exists() {
        return Ok((hash, false));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let payload = if crate::secrets::master_key().len() == 32 {
        crate::secrets::encrypt_bytes(&format!("chunk:{instance_id}"), data)
    } else {
        data.to_vec()
    };
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&payload)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok((hash, true))
}

pub fn load_object(instance_id: &str, hash: &str) -> anyhow::Result<Vec<u8>> {
    let path = object_path(instance_id, hash);
    let raw = fs::read(&path)?;
    if crate::secrets::is_encrypted_bytes(&raw) {
        let plain = crate::secrets::decrypt_bytes(&format!("chunk:{instance_id}"), &raw)
            .ok_or_else(|| anyhow::anyhow!("备份对象解密失败: {hash}"))?;
        if crypto::sha256_hex(&plain) != hash {
            anyhow::bail!("备份对象校验失败: {hash}");
        }
        return Ok(plain);
    }
    if crypto::sha256_hex(&raw) != hash {
        anyhow::bail!("备份对象校验失败: {hash}");
    }
    Ok(raw)
}

fn walk_files(root: &Path) -> anyhow::Result<Vec<(String, PathBuf, u64, u32)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                let name = path.file_name().map(|s| s.to_string_lossy().to_string());
                if name.as_deref() == Some("session.lock") {
                    continue;
                }
                stack.push(path);
            } else if meta.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = 0o644u32;
                out.push((rel, path, meta.len(), mode));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn chain_of(instance_id: &str, parent: Option<&str>) -> String {
    let parent_chain = parent
        .and_then(|p| read_snapshot(instance_id, p).ok().flatten())
        .map(|s| s.chain)
        .unwrap_or_default();
    let seed = format!("{parent_chain}|{instance_id}");
    crypto::sha256_hex(seed.as_bytes())[..16].to_string()
}

pub fn create_snapshot(
    instance_id: &str,
    workdir: &str,
    label: &str,
) -> anyhow::Result<Snapshot> {
    let root = Path::new(workdir);
    if !root.is_dir() {
        anyhow::bail!("实例目录不存在: {workdir}");
    }
    let files = walk_files(root)?;
    let mut metas = Vec::with_capacity(files.len());
    let mut total = 0u64;
    let mut stored = 0u64;
    let mut chunk_total = 0usize;
    for (rel, path, size, mode) in files {
        let mut f = fs::File::open(&path)?;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut chunks = Vec::new();
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let (hash, created) = store_object(instance_id, &buf[..n])?;
            if created {
                stored += n as u64;
            }
            chunks.push(hash);
            chunk_total += 1;
        }
        if chunks.is_empty() {
            let (hash, created) = store_object(instance_id, &[])?;
            if created {
                stored += 0;
            }
            chunks.push(hash);
            chunk_total += 1;
        }
        total += size;
        metas.push(FileEntryMeta {
            path: rel,
            size,
            chunks,
            mode,
        });
    }
    let now = Utc::now();
    let parent = latest_snapshot_id(instance_id)?;
    let id = format!("{}", now.format("%Y%m%d-%H%M%S%.3f"));
    let chain = chain_of(instance_id, parent.as_deref());
    let snapshot = Snapshot {
        id: id.clone(),
        instance_id: instance_id.to_string(),
        created_at: now,
        label: label.to_string(),
        files: metas,
        total_bytes: total,
        stored_bytes: stored,
        chunk_count: chunk_total,
        parent,
        chain,
    };
    write_snapshot(&snapshot)?;
    Ok(snapshot)
}

pub fn manifest_digest(snapshot: &Snapshot) -> String {
    let mut canon = String::new();
    canon.push_str(&snapshot.chain);
    canon.push('|');
    canon.push_str(&snapshot.id);
    canon.push('|');
    canon.push_str(&snapshot.instance_id);
    canon.push('|');
    canon.push_str(&snapshot.created_at.to_rfc3339());
    canon.push('|');
    canon.push_str(&snapshot.total_bytes.to_string());
    canon.push('|');
    for f in &snapshot.files {
        canon.push_str(&f.path);
        canon.push(':');
        canon.push_str(&f.size.to_string());
        canon.push(':');
        canon.push_str(&f.mode.to_string());
        canon.push(':');
        for c in &f.chunks {
            canon.push_str(c);
            canon.push(',');
        }
        canon.push(';');
    }
    crypto::sha256_hex(canon.as_bytes())
}

fn write_snapshot(snapshot: &Snapshot) -> anyhow::Result<()> {
    let dir = snapshots_dir(&snapshot.instance_id);
    fs::create_dir_all(&dir)?;
    let path = snapshot_path(&snapshot.instance_id, &snapshot.id);
    let digest = manifest_digest(snapshot);
    let mut obj = serde_json::to_value(snapshot)?;
    if let Some(map) = obj.as_object_mut() {
        map.insert("manifest_digest".into(), serde_json::Value::String(digest));
    }
    let text = serde_json::to_string_pretty(&obj)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read_snapshot(instance_id: &str, id: &str) -> anyhow::Result<Option<Snapshot>> {
    let path = snapshot_path(instance_id, id);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let snap: Snapshot = serde_json::from_str(&text)?;
    Ok(Some(snap))
}

pub fn list_snapshots(instance_id: &str) -> anyhow::Result<Vec<Snapshot>> {
    let dir = snapshots_dir(instance_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(snap) = serde_json::from_str::<Snapshot>(&text) {
                    out.push(snap);
                }
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(out)
}

pub fn latest_snapshot_id(instance_id: &str) -> anyhow::Result<Option<String>> {
    let list = list_snapshots(instance_id)?;
    Ok(list.last().map(|s| s.id.clone()))
}

pub fn verify_manifest(instance_id: &str, id: &str) -> anyhow::Result<(usize, usize)> {
    let snap = read_snapshot(instance_id, id)?
        .ok_or_else(|| anyhow::anyhow!("快照不存在: {id}"))?;
    let path = snapshot_path(instance_id, id);
    let text = fs::read_to_string(&path)?;
    let expect = manifest_digest(&snap);
    let obj: serde_json::Value = serde_json::from_str(&text)?;
    if let Some(digest) = obj.get("manifest_digest").and_then(|v| v.as_str()) {
        if digest != expect {
            anyhow::bail!("清单摘要不匹配, 快照可能被篡改: {id}");
        }
    }
    let mut checked = 0usize;
    let mut missing = 0usize;
    let mut seen = BTreeSet::new();
    for f in &snap.files {
        for c in &f.chunks {
            if !seen.insert(c.clone()) {
                continue;
            }
            checked += 1;
            if !object_path(instance_id, c).exists() {
                missing += 1;
            }
        }
    }
    Ok((checked, missing))
}

pub fn restore_snapshot(
    instance_id: &str,
    id: &str,
    dest: &str,
    prune_extra: bool,
) -> anyhow::Result<u64> {
    let snap = read_snapshot(instance_id, id)?
        .ok_or_else(|| anyhow::anyhow!("快照不存在: {id}"))?;
    let dest_root = Path::new(dest);
    fs::create_dir_all(dest_root)?;
    let mut written = 0u64;
    let mut expected = BTreeSet::new();
    for f in &snap.files {
        expected.insert(f.path.clone());
        let target = dest_root.join(&f.path);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut out = fs::File::create(&target)?;
        for c in &f.chunks {
            let data = load_object(instance_id, c)?;
            out.write_all(&data)?;
            written += data.len() as u64;
        }
        out.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if f.mode == 0 { 0o644 } else { f.mode & 0o777 };
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }
    }
    if prune_extra {
        for (rel, path, _, _) in walk_files(dest_root)? {
            if !expected.contains(&rel) {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(written)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePoint {
    pub id: String,
    pub at: DateTime<Utc>,
    pub snapshot_id: String,
    pub reason: String,
}

pub fn record_restore_point(
    instance_id: &str,
    snapshot_id: &str,
    reason: &str,
) -> anyhow::Result<RestorePoint> {
    let point = RestorePoint {
        id: crypto::random_token(12),
        at: Utc::now(),
        snapshot_id: snapshot_id.to_string(),
        reason: reason.to_string(),
    };
    let dir = backup_root(instance_id).join("points");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", point.id));
    fs::write(&path, serde_json::to_string_pretty(&point)?)?;
    Ok(point)
}

pub fn list_restore_points(instance_id: &str) -> anyhow::Result<Vec<RestorePoint>> {
    let dir = backup_root(instance_id).join("points");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if let Ok(text) = fs::read_to_string(entry.path()) {
            if let Ok(p) = serde_json::from_str::<RestorePoint>(&text) {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.at.cmp(&b.at));
    Ok(out)
}

pub fn point_before(instance_id: &str, at: DateTime<Utc>) -> anyhow::Result<Option<RestorePoint>> {
    let points = list_restore_points(instance_id)?;
    Ok(points.into_iter().filter(|p| p.at < at).next_back())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub hourly: u32,
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            hourly: 24,
            daily: 7,
            weekly: 4,
            monthly: 12,
        }
    }
}

fn bucket_key(t: DateTime<Utc>, kind: &str) -> String {
    match kind {
        "hourly" => t.format("%Y%m%d%H").to_string(),
        "daily" => t.format("%Y%m%d").to_string(),
        "weekly" => format!("{}-W{}", t.iso_week().year(), t.iso_week().week()),
        _ => t.format("%Y%m").to_string(),
    }
}

pub fn gfs_keep_set(
    snapshots: &[Snapshot],
    policy: &RetentionPolicy,
) -> Vec<String> {
    let mut keep: BTreeSet<String> = BTreeSet::new();
    let mut sorted: Vec<&Snapshot> = snapshots.iter().collect();
    sorted.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let ordered: Vec<&Snapshot> = sorted;
    for (kind, limit) in [
        ("hourly", policy.hourly),
        ("daily", policy.daily),
        ("weekly", policy.weekly),
        ("monthly", policy.monthly),
    ] {
        if limit == 0 {
            continue;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut picked = 0u32;
        for snap in ordered.iter().rev() {
            let key = bucket_key(snap.created_at, kind);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            keep.insert(snap.id.clone());
            picked += 1;
            if picked >= limit {
                break;
            }
        }
    }
    if let Some(last) = ordered.last() {
        keep.insert(last.id.clone());
    }
    let mut out: Vec<String> = keep.into_iter().collect();
    out.sort();
    out
}

pub fn prune_gfs(
    instance_id: &str,
    policy: &RetentionPolicy,
) -> anyhow::Result<(usize, usize)> {
    let snapshots = list_snapshots(instance_id)?;
    if snapshots.is_empty() {
        return Ok((0, 0));
    }
    let keep = gfs_keep_set(&snapshots, policy);
    let mut removed = 0usize;
    for snap in &snapshots {
        if !keep.contains(&snap.id) {
            let _ = fs::remove_file(snapshot_path(instance_id, &snap.id));
            removed += 1;
        }
    }
    let reachable = reachable_objects(instance_id)?;
    let mut deleted = 0usize;
    for obj in list_objects(instance_id)? {
        if !reachable.contains(&obj) {
            let path = object_path(instance_id, &obj);
            if fs::remove_file(&path).is_ok() {
                deleted += 1;
            }
        }
    }
    Ok((removed, deleted))
}

fn list_objects(instance_id: &str) -> anyhow::Result<Vec<String>> {
    let root = objects_dir(instance_id);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for prefix in fs::read_dir(&root)? {
        let prefix = prefix?;
        if !prefix.path().is_dir() {
            continue;
        }
        for obj in fs::read_dir(prefix.path())? {
            let obj = obj?;
            if let Some(name) = obj.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    Ok(out)
}

pub fn reachable_objects(instance_id: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for snap in list_snapshots(instance_id)? {
        for f in &snap.files {
            for c in &f.chunks {
                out.insert(c.clone());
            }
        }
    }
    Ok(out)
}

pub fn store_stats(instance_id: &str) -> anyhow::Result<StoreStats> {
    let snapshots = list_snapshots(instance_id)?;
    let objects = list_objects(instance_id)?;
    let mut object_bytes = 0u64;
    for obj in &objects {
        if let Ok(meta) = fs::metadata(object_path(instance_id, obj)) {
            object_bytes += meta.len();
        }
    }
    Ok(StoreStats {
        objects: objects.len(),
        object_bytes,
        snapshots: snapshots.len(),
        logical_bytes: snapshots.iter().map(|s| s.total_bytes).sum(),
    })
}

pub fn dedup_ratio(instance_id: &str) -> anyhow::Result<f64> {
    let stats = store_stats(instance_id)?;
    if stats.object_bytes == 0 {
        return Ok(0.0);
    }
    Ok(stats.logical_bytes as f64 / stats.object_bytes as f64)
}

pub fn diff_snapshots(
    instance_id: &str,
    from: &str,
    to: &str,
) -> anyhow::Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let a = read_snapshot(instance_id, from)?
        .ok_or_else(|| anyhow::anyhow!("快照不存在: {from}"))?;
    let b = read_snapshot(instance_id, to)?
        .ok_or_else(|| anyhow::anyhow!("快照不存在: {to}"))?;
    let map_a: BTreeMap<String, &FileEntryMeta> =
        a.files.iter().map(|f| (f.path.clone(), f)).collect();
    let map_b: BTreeMap<String, &FileEntryMeta> =
        b.files.iter().map(|f| (f.path.clone(), f)).collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (path, fb) in &map_b {
        match map_a.get(path) {
            None => added.push(path.clone()),
            Some(fa) => {
                if fa.chunks != fb.chunks {
                    changed.push(path.clone());
                }
            }
        }
    }
    for path in map_a.keys() {
        if !map_b.contains_key(path) {
            removed.push(path.clone());
        }
    }
    Ok((added, removed, changed))
}

pub fn rebuild_chain(instance_id: &str) -> anyhow::Result<usize> {
    let mut snaps = list_snapshots(instance_id)?;
    if snaps.is_empty() {
        return Ok(0);
    }
    snaps.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut parent: Option<String> = None;
    let mut n = 0;
    for mut snap in snaps {
        snap.parent = parent.clone();
        snap.chain = chain_of(instance_id, parent.as_deref());
        write_snapshot(&snap)?;
        parent = Some(snap.id.clone());
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ck-bk-{tag}-{}",
            crypto::random_token(8)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn set_data_root(dir: &Path) {
        std::env::set_current_dir(dir).unwrap();
    }

    #[test]
    fn snapshot_dedup_and_restore() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = temp_dir("snap");
        let prev = std::env::current_dir().unwrap();
        set_data_root(&root);
        let wd = root.join("srv");
        fs::create_dir_all(&wd).unwrap();
        fs::write(wd.join("a.txt"), b"hello world").unwrap();
        fs::write(wd.join("b.txt"), vec![7u8; 4096]).unwrap();
        let s1 = create_snapshot("inst1", wd.to_str().unwrap(), "first").unwrap();
        assert_eq!(s1.files.len(), 2);
        let stats1 = store_stats("inst1").unwrap();
        fs::write(wd.join("a.txt"), b"hello world").unwrap();
        let s2 = create_snapshot("inst1", wd.to_str().unwrap(), "second").unwrap();
        let stats2 = store_stats("inst1").unwrap();
        assert_eq!(stats2.objects, stats1.objects);
        assert!(s2.stored_bytes < s1.total_bytes);
        let (checked, missing) = verify_manifest("inst1", &s2.id).unwrap();
        assert!(checked > 0);
        assert_eq!(missing, 0);
        let out = root.join("restored");
        restore_snapshot("inst1", &s1.id, out.to_str().unwrap(), true).unwrap();
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"hello world");
        assert_eq!(fs::read(out.join("b.txt")).unwrap(), vec![7u8; 4096]);
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn gfs_retention_keeps_buckets() {
        let mut snaps = Vec::new();
        for i in 0..40 {
            snaps.push(Snapshot {
                id: format!("s{i:02}"),
                instance_id: "i".into(),
                created_at: Utc::now() - chrono::Duration::hours((39 - i) as i64),
                label: String::new(),
                files: Vec::new(),
                total_bytes: 0,
                stored_bytes: 0,
                chunk_count: 0,
                parent: None,
                chain: String::new(),
            });
        }
        let policy = RetentionPolicy {
            hourly: 6,
            daily: 3,
            weekly: 2,
            monthly: 1,
        };
        let keep = gfs_keep_set(&snaps, &policy);
        assert!(keep.contains(&"s39".to_string()));
        assert!(keep.len() < snaps.len());
        assert!(keep.len() >= 6);
    }

    #[test]
    fn diff_reports_changes() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = temp_dir("diff");
        let prev = std::env::current_dir().unwrap();
        set_data_root(&root);
        let wd = root.join("srv");
        fs::create_dir_all(&wd).unwrap();
        fs::write(wd.join("keep.txt"), b"same").unwrap();
        fs::write(wd.join("gone.txt"), b"bye").unwrap();
        let a = create_snapshot("i2", wd.to_str().unwrap(), "").unwrap();
        fs::remove_file(wd.join("gone.txt")).unwrap();
        fs::write(wd.join("keep.txt"), b"changed").unwrap();
        fs::write(wd.join("new.txt"), b"hi").unwrap();
        let b = create_snapshot("i2", wd.to_str().unwrap(), "").unwrap();
        let (added, removed, changed) = diff_snapshots("i2", &a.id, &b.id).unwrap();
        assert_eq!(added, vec!["new.txt".to_string()]);
        assert_eq!(removed, vec!["gone.txt".to_string()]);
        assert_eq!(changed, vec!["keep.txt".to_string()]);
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn tamper_detection_on_manifest() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = temp_dir("tamper");
        let prev = std::env::current_dir().unwrap();
        set_data_root(&root);
        let wd = root.join("srv");
        fs::create_dir_all(&wd).unwrap();
        fs::write(wd.join("x.txt"), b"data").unwrap();
        let s = create_snapshot("i3", wd.to_str().unwrap(), "").unwrap();
        assert!(verify_manifest("i3", &s.id).is_ok());
        let path = snapshot_path("i3", &s.id);
        let text = fs::read_to_string(&path).unwrap();
        let mut val: serde_json::Value = serde_json::from_str(&text).unwrap();
        if let Some(files) = val.get_mut("files").and_then(|f| f.as_array_mut()) {
            if let Some(first) = files.first_mut() {
                first["size"] = serde_json::json!(999999);
            }
        }
        fs::write(&path, serde_json::to_string_pretty(&val).unwrap()).unwrap();
        assert!(verify_manifest("i3", &s.id).is_err());
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn restore_points_ordering() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let root = temp_dir("points");
        let prev = std::env::current_dir().unwrap();
        set_data_root(&root);
        let p1 = record_restore_point("i4", "s1", "manual").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let p2 = record_restore_point("i4", "s2", "auto").unwrap();
        let list = list_restore_points("i4").unwrap();
        assert_eq!(list.len(), 2);
        let before = point_before("i4", p2.at).unwrap().unwrap();
        assert_eq!(before.id, p1.id);
        std::env::set_current_dir(prev).unwrap();
    }
}