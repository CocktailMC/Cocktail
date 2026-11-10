use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::handlers::ErrorBody;
use crate::state::SharedState;
use crate::tutorials::{
    self, Catalog, Progress, TutorialView, WatchSummary, build_views, summarize,
};

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub tutorials: Vec<TutorialView>,
    pub summary: WatchSummary,
}

#[derive(Deserialize)]
pub struct ProgressBody {
    pub position_secs: u32,
    pub duration_secs: u32,
}

#[derive(Serialize)]
pub struct ProgressResponse {
    pub id: String,
    pub progress: Progress,
    pub percent: u32,
    pub completed: bool,
}

fn store() -> &'static Mutex<BTreeMap<String, BTreeMap<String, Progress>>> {
    static S: OnceLock<Mutex<BTreeMap<String, BTreeMap<String, Progress>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn catalog() -> Catalog {
    let path = std::env::var("COCKTAIL_TUTORIALS").ok();
    if let Some(p) = path {
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(parsed) = serde_json::from_str::<Catalog>(&text) {
                return parsed;
            }
        }
    }
    Catalog::defaults()
}

fn user_progress(user: &str) -> BTreeMap<String, Progress> {
    store()
        .lock()
        .map(|g| g.get(user).cloned().unwrap_or_default())
        .unwrap_or_default()
}

pub async fn list_tutorials(
    Query(q): Query<ListQuery>,
) -> Json<ListResponse> {
    let cat = catalog();
    let views = if let Some(needle) = q.q.as_deref() {
        let hits = cat.search(needle);
        let mut filtered = Catalog {
            tutorials: hits.into_iter().cloned().collect(),
        };
        if filtered.tutorials.is_empty() {
            filtered = Catalog {
                tutorials: Vec::new(),
            };
        }
        build_views(&filtered, &BTreeMap::new())
    } else {
        build_views(&cat, &BTreeMap::new())
    };
    let summary = summarize(&views);
    Json(ListResponse {
        tutorials: views,
        summary,
    })
}

pub async fn my_tutorials(
    State(_state): State<SharedState>,
) -> Json<ListResponse> {
    let cat = catalog();
    let progress = user_progress("local");
    let views = build_views(&cat, &progress);
    let summary = summarize(&views);
    Json(ListResponse {
        tutorials: views,
        summary,
    })
}

pub async fn report_progress(
    Path(id): Path<String>,
    Json(body): Json<ProgressBody>,
) -> Result<Json<ProgressResponse>, (StatusCode, Json<ErrorBody>)> {
    let cat = catalog();
    let Some(tut) = cat.find(&id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("教程不存在: {id}"),
            }),
        ));
    };
    let duration = if body.duration_secs > 0 {
        body.duration_secs
    } else {
        tut.duration_secs
    };
    let mut guard = match store().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let entry = guard.entry("local".to_string()).or_default();
    let p = tutorials::apply_progress(entry, &id, body.position_secs, duration);
    let percent = p.percent(duration);
    let completed = p.is_complete(duration);
    Ok(Json(ProgressResponse {
        id,
        progress: p,
        percent,
        completed,
    }))
}
