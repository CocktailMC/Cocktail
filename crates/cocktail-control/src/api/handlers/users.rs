use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::api::handlers::{ErrorBody, bad_request, current_admin, not_found};
use crate::state::SharedState;

#[derive(Serialize)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateUserBody {
    pub username: String,
    pub password: String,
    pub role: String,
}

fn normalize_role(raw: &str) -> anyhow::Result<String> {
    Ok(match raw.trim().to_ascii_lowercase().as_str() {
        "owner" | "superadmin" => "superadmin".into(),
        "admin" | "管理员" => "admin".into(),
        "support" | "客服" => "support".into(),
        "developer" | "dev" | "开发" => "developer".into(),
        "observer" | "观察员" => "observer".into(),
        _ => anyhow::bail!("未知角色（owner / admin / support / developer / observer）"),
    })
}

async fn session_admin(
    state: &SharedState,
    headers: &HeaderMap,
) -> Result<crate::db::AdminRow, (StatusCode, Json<ErrorBody>)> {
    let admin = current_admin(state, headers).await?;
    if !crate::auth::can(&admin.role, "view") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "权限不足".into(),
            }),
        ));
    }
    Ok(admin)
}

fn require_perm(
    admin: &crate::db::AdminRow,
    perm: &str,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if crate::auth::can(&admin.role, perm) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            error: format!("权限不足：需要 {perm} 权限（当前角色 {}）", admin.role),
        }),
    ))
}

pub async fn list_users(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let admin = session_admin(&state, &headers).await?;
    require_perm(&admin, "users")?;
    let conn = state.db.get().expect("db pool");
    let rows = crate::db::list_admins(&conn).map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(
        rows.into_iter()
            .map(|a| UserView {
                id: a.id,
                username: a.username,
                role: a.role,
                created_at: a.created_at,
            })
            .collect::<Vec<_>>(),
    ))
}

pub async fn create_user(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let admin = session_admin(&state, &headers).await?;
    require_perm(&admin, "users")?;
    let username =
        crate::auth::validate_username(&body.username).map_err(|e| bad_request(e.to_string()))?;
    crate::auth::validate_password(&body.password).map_err(|e| bad_request(e.to_string()))?;
    let role = normalize_role(&body.role).map_err(|e| bad_request(e.to_string()))?;
    if role == "superadmin" && admin.role != "superadmin" {
        return Err(bad_request("只有 Owner 可以创建 Owner"));
    }
    let hash =
        crate::auth::hash_password(&body.password).map_err(|e| bad_request(e.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let conn = state.db.get().expect("db pool");
    crate::db::insert_admin(&conn, &username, &hash, &role, &now)
        .map_err(|e| bad_request(e.to_string()))?;
    crate::util::audit(
        "user.create",
        None,
        serde_json::json!({ "username": username, "role": role }),
        &admin.username,
    );
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
pub struct UpdateUserBody {
    pub role: Option<String>,
    pub password: Option<String>,
}

pub async fn update_user(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let admin = session_admin(&state, &headers).await?;
    require_perm(&admin, "users")?;
    let conn = state.db.get().expect("db pool");
    let target = crate::db::admin_by_id(&conn, id)
        .map_err(|e| bad_request(e.to_string()))?
        .ok_or_else(|| not_found("用户不存在"))?;
    let mut changed: Vec<String> = Vec::new();
    if let Some(role_raw) = body.role.as_deref() {
        let role = normalize_role(role_raw).map_err(|e| bad_request(e.to_string()))?;
        if role == "superadmin" && admin.role != "superadmin" {
            return Err(bad_request("只有 Owner 可以授予 Owner"));
        }
        if admin.id == id && role != admin.role {
            return Err(bad_request("不能修改自己的角色"));
        }
        crate::db::update_admin_role(&conn, id, &role).map_err(|e| bad_request(e.to_string()))?;
        let _ = crate::db::delete_sessions_for_admin(&conn, id);
        changed.push(format!("role={role}"));
    }
    if let Some(pw) = body.password.as_deref().filter(|s| !s.is_empty()) {
        if admin.role != "superadmin" && admin.id != id {
            return Err(bad_request("只有 Owner 可以重置他人密码"));
        }
        crate::auth::validate_password(pw).map_err(|e| bad_request(e.to_string()))?;
        let hash = crate::auth::hash_password(pw).map_err(|e| bad_request(e.to_string()))?;
        crate::db::update_admin(&conn, id, None, Some(&hash))
            .map_err(|e| bad_request(e.to_string()))?;
        let _ = crate::db::delete_sessions_for_admin(&conn, id);
        changed.push("password".into());
    }
    crate::util::audit(
        "user.update",
        None,
        serde_json::json!({ "id": id, "target": target.username, "changed": changed }),
        &admin.username,
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_user(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let admin = session_admin(&state, &headers).await?;
    require_perm(&admin, "users")?;
    if admin.id == id {
        return Err(bad_request("不能删除当前登录账号"));
    }
    let conn = state.db.get().expect("db pool");
    crate::db::delete_admin(&conn, id).map_err(|e| bad_request(e.to_string()))?;
    crate::util::audit(
        "user.delete",
        None,
        serde_json::json!({ "id": id }),
        &admin.username,
    );
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{request, setup_and_login};
    use crate::state::test_state;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn users_require_auth() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let (s, _) = request(&state, "GET", "/api/v1/users", None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn user_crud_as_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let token = setup_and_login(&state).await;

        // list starts with the super-admin only
        let (s, v) = request(&state, "GET", "/api/v1/users", Some(&token), None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);

        // create a developer
        let (s, _) = request(
            &state,
            "POST",
            "/api/v1/users",
            Some(&token),
            Some(serde_json::json!({
                "username": "dev1",
                "password": "DevPass!123",
                "role": "developer",
            })),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED);

        // promote to admin
        let dev_id = {
            let (_, v) = request(&state, "GET", "/api/v1/users", Some(&token), None).await;
            v.as_array()
                .unwrap()
                .iter()
                .find(|u| u["username"] == "dev1")
                .expect("dev1 listed")["id"]
                .as_i64()
                .unwrap()
        };
        let (s, _) = request(
            &state,
            "PUT",
            &format!("/api/v1/users/{dev_id}"),
            Some(&token),
            Some(serde_json::json!({ "role": "admin" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        // delete
        let (s, _) = request(
            &state,
            "DELETE",
            &format!("/api/v1/users/{dev_id}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        // back to one user
        let (_, v) = request(&state, "GET", "/api/v1/users", Some(&token), None).await;
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn cannot_delete_last_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp.path().join("t.db")).await;
        let token = setup_and_login(&state).await;
        let owner_id = {
            let (_, v) = request(&state, "GET", "/api/v1/users", Some(&token), None).await;
            v.as_array().unwrap()[0]["id"].as_i64().unwrap()
        };
        let (s, _) = request(
            &state,
            "DELETE",
            &format!("/api/v1/users/{owner_id}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
