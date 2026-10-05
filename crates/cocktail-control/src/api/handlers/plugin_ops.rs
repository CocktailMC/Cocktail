use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::api::handlers::{ErrorBody, map_result, not_found};
use crate::instance::{
    self, HangarVersionsQuery, InstallHangarRequest, InstallModrinthRequest, InstallRequest,
    InstallSpigetRequest, ModrinthVersionsQuery, SearchQuery,
};
use crate::state::SharedState;

pub async fn list_plugins(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_plugins(&state, &id).await)
}

pub async fn enable_plugin(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_plugin_enabled(&state, &id, &name, true).await)
}

pub async fn disable_plugin(
    State(state): State<SharedState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::set_plugin_enabled(&state, &id, &name, false).await)
}

pub async fn list_core_versions(
    Path(core): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_versions(&core).await)
}

pub async fn list_core_loaders(
    Path((core, version)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::list_loaders(&core, &version).await)
}

pub async fn install_core(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_core(&state, &id, req).await)
}

pub async fn modrinth_search(
    Query(q): Query<SearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::modrinth::search(&q).await)
}

pub async fn modrinth_versions(
    Path(id): Path<String>,
    Query(q): Query<ModrinthVersionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::modrinth::list_versions(&id, &q).await)
}

pub async fn modrinth_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallModrinthRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_modrinth(&state, &id, req).await)
}

#[derive(Deserialize)]
pub struct HangarSearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_20")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_paper")]
    pub platform: String,
}

fn default_20() -> u32 {
    20
}

fn default_paper() -> String {
    "PAPER".into()
}

pub async fn hangar_search(
    Query(q): Query<HangarSearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(
        instance::hangar::search(&instance::hangar::SearchQuery {
            query: q.query,
            limit: q.limit,
            offset: q.offset,
            platform: q.platform,
        })
        .await,
    )
}

pub async fn hangar_versions(
    Path(slug): Path<String>,
    Query(q): Query<HangarVersionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::hangar::list_versions(&slug, &q).await)
}

pub async fn hangar_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallHangarRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_hangar(&state, &id, req).await)
}

#[derive(Deserialize)]
pub struct SpigetSearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_20")]
    pub size: u32,
    #[serde(default)]
    pub page: u32,
}

pub async fn spiget_search(
    Query(q): Query<SpigetSearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(
        instance::spiget::search(&instance::spiget::SearchQuery {
            query: q.query,
            size: q.size,
            page: q.page,
        })
        .await,
    )
}

pub async fn spiget_versions(
    Path(rid): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::spiget::list_versions(rid).await)
}

pub async fn spiget_icon(Path(rid): Path<i64>) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    match instance::spiget::fetch_icon(rid).await {
        Ok((ctype, bytes)) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctype)
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(bytes))
            .unwrap()),
        Err(e) => Err(not_found(e.to_string())),
    }
}

pub async fn spiget_install(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<InstallSpigetRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    map_result(instance::install_spiget(&state, &id, req).await)
}
