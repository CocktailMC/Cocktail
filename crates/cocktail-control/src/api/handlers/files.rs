use axum::Json;
use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::api::handlers::{ErrorBody, bad_request, map_result, not_found};
use crate::instance::{self, WriteFileRequest};
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

#[derive(Deserialize)]
pub struct UploadQuery {
    path: String,
}

pub async fn list_files(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_files(&state, &id, &q.path).await)
}

pub async fn read_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::read_file(&state, &id, &q.path).await)
}

pub async fn write_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<WriteFileRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::write_file(&state, &id, &req.path, &req.content).await)
}

pub async fn delete_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    match instance::delete_file(&state, &id, &q.path).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) if e.to_string().contains("not found") => Err(not_found(e.to_string())),
        Err(e) => Err(bad_request(e.to_string())),
    }
}

pub async fn download_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let (path, bytes) = instance::read_bytes(&state, &id, &q.path)
        .await
        .map_err(|e| {
            if e.to_string().contains("not found") {
                not_found(e.to_string())
            } else {
                bad_request(e.to_string())
            }
        })?;
    let filename = path.rsplit('/').next().unwrap_or("download");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(bytes))
        .unwrap())
}

pub async fn upload_file(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") || bytes.is_none() {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?
                    .to_vec(),
            );
        }
    }
    let bytes = bytes.ok_or_else(|| bad_request("missing file field"))?;
    map_result(instance::write_bytes(&state, &id, &q.path, &bytes).await)
}

#[derive(Deserialize)]
pub struct MkdirBody {
    pub path: String,
}

pub async fn mkdir(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<MkdirBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::mkdir(&state, &id, &body.path).await)
}

#[derive(Deserialize)]
pub struct InstallJarQuery {
    #[serde(default = "default_server_jar")]
    pub path: String,
    #[serde(default = "default_custom_core")]
    pub core: String,
    #[serde(default = "default_true_bool")]
    pub accept_eula: bool,
}

fn default_server_jar() -> String {
    "server.jar".into()
}

fn default_custom_core() -> String {
    "custom".into()
}

fn default_true_bool() -> bool {
    true
}

pub async fn install_jar(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<InstallJarQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") || bytes.is_none() {
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?
                    .to_vec(),
            );
        }
    }
    let bytes = bytes.ok_or_else(|| bad_request("missing file field"))?;
    map_result(
        instance::install_local_jar(&state, &id, &q.path, &bytes, Some(q.core), q.accept_eula)
            .await,
    )
}

#[derive(Deserialize)]
pub struct ImportArchiveQuery {
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default = "default_custom_core")]
    pub core: String,
    #[serde(default = "default_true_bool")]
    pub accept_eula: bool,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
}

pub async fn import_archive(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<ImportArchiveQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let tmp_root = std::path::PathBuf::from("data").join("tmp").join("imports");
    tokio::fs::create_dir_all(&tmp_root)
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    let staging = tmp_root.join(format!("{}-{}", id, uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| bad_request(e.to_string()))?;

    let saved = save_multipart_archive(&mut multipart, &staging, q.filename.as_deref()).await;
    let saved = match saved {
        Ok(v) => v,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return Err(bad_request(e.to_string()));
        }
    };

    let args = q
        .args
        .as_deref()
        .map(|s| {
            s.split_whitespace()
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let opts = instance::ImportArchiveOpts {
        filename: saved.filename,
        accept_eula: q.accept_eula,
        core: Some(q.core),
        command: q.command.filter(|s| !s.trim().is_empty()),
        args,
    };
    let result: crate::instance::archive::ImportArchiveResult =
        match instance::import_archive(&state, &id, &saved.path, opts).await {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&staging).await;
                return map_result(Err(e));
            }
        };
    let _ = tokio::fs::remove_dir_all(&staging).await;
    map_result(Ok(result))
}

struct SavedArchive {
    path: std::path::PathBuf,
    filename: String,
}

async fn save_multipart_archive(
    multipart: &mut Multipart,
    staging: &std::path::Path,
    hint: Option<&str>,
) -> anyhow::Result<SavedArchive> {
    let mut saved: Option<SavedArchive> = None;
    while let Some(mut field) = multipart.next_field().await? {
        let orig = field
            .file_name()
            .map(|s| s.to_string())
            .or_else(|| hint.map(|s| s.to_string()))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "pack.bin".into());
        let safe = orig
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("pack.bin")
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>();
        if !crate::sevenz::is_supported_name(&safe) {
            anyhow::bail!("不支持的压缩格式：{safe}（支持 7z / zip / tar.gz / tar.xz）");
        }
        let dest = staging.join(&safe);
        let mut file = tokio::fs::File::create(&dest).await?;
        let mut written: u64 = 0;
        while let Some(chunk) = field.chunk().await? {
            written += chunk.len() as u64;
            if written > instance::MAX_ARCHIVE_BYTES {
                anyhow::bail!("压缩包超过 2GiB 上限");
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }
        drop(file);
        if written == 0 {
            anyhow::bail!("上传文件为空");
        }
        saved = Some(SavedArchive {
            path: dest,
            filename: safe,
        });
    }
    saved.ok_or_else(|| anyhow::anyhow!("missing file field"))
}

#[derive(Deserialize)]
pub struct StartupJarBody {
    pub path: String,
}

pub async fn set_startup_jar(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<StartupJarBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_startup_jar(&state, &id, &body.path).await)
}

#[cfg(test)]
mod tests {
    use super::super::testutil::request;
    use crate::state::test_state;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn missing_instance_files_404() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let (s, _) = request(
            &state,
            "GET",
            "/api/v1/instances/nope/files?path=.",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn write_read_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;

        // create an instance (creates a real workdir under data/instances/<id>)
        let (s, v) = request(
            &state,
            "POST",
            "/api/v1/instances",
            None,
            Some(serde_json::json!({ "name": "FileSrv", "port": 25598 })),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED);
        let id = v["id"].as_str().unwrap().to_string();

        // write a file
        let (s, _) = request(
            &state,
            "PUT",
            &format!("/api/v1/instances/{id}/files/content"),
            None,
            Some(serde_json::json!({ "path": "greeting.txt", "content": "hello cockpit" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "write_file should succeed");

        // read it back
        let (s, v) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{id}/files/content?path=greeting.txt"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["content"], "hello cockpit");

        // list root dir contains it
        let (s, v) = request(
            &state,
            "GET",
            &format!("/api/v1/instances/{id}/files?path=."),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert!(names.contains(&"greeting.txt"));

        // delete it
        let (s, _) = request(
            &state,
            "DELETE",
            &format!("/api/v1/instances/{id}/files/content?path=greeting.txt"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
    }
}
