use serde_json::{Value, json};

struct Route {
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    summary: &'static str,
    perm: &'static str,
    mutation: bool,
}

const ROUTES: &[Route] = &[
    Route {
        method: "get",
        path: "/api/v1/health",
        tag: "system",
        summary: "存活探针",
        perm: "public",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/ready",
        tag: "system",
        summary: "就绪探针",
        perm: "public",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/metrics",
        tag: "system",
        summary: "Prometheus 指标",
        perm: "metrics.read",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/setup",
        tag: "auth",
        summary: "初始化最高管理员",
        perm: "public",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/auth/login",
        tag: "auth",
        summary: "登录",
        perm: "public",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/auth/logout",
        tag: "auth",
        summary: "登出",
        perm: "view",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/auth/me",
        tag: "auth",
        summary: "当前会话",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "put",
        path: "/api/v1/auth/password",
        tag: "auth",
        summary: "修改密码",
        perm: "view",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/auth/2fa/setup",
        tag: "auth",
        summary: "生成 TOTP 密钥",
        perm: "2fa.manage",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/auth/2fa/verify",
        tag: "auth",
        summary: "校验并开启 2FA",
        perm: "2fa.manage",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/auth/2fa/status",
        tag: "auth",
        summary: "2FA 状态",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/auth/2fa/disable",
        tag: "auth",
        summary: "关闭 2FA",
        perm: "2fa.manage",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances",
        tag: "instances",
        summary: "实例列表",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/instances",
        tag: "instances",
        summary: "创建实例",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}",
        tag: "instances",
        summary: "实例详情",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "put",
        path: "/api/v1/instances/{id}",
        tag: "instances",
        summary: "更新实例",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/instances/{id}",
        tag: "instances",
        summary: "删除实例",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/start",
        tag: "instances",
        summary: "启动",
        perm: "start",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/stop",
        tag: "instances",
        summary: "停止",
        perm: "stop",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/restart",
        tag: "instances",
        summary: "重启",
        perm: "stop",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/command",
        tag: "instances",
        summary: "发送控制台命令",
        perm: "console",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/clone",
        tag: "instances",
        summary: "克隆实例",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/preflight",
        tag: "instances",
        summary: "启动预检",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/rcon/status",
        tag: "rcon",
        summary: "RCON 状态",
        perm: "rcon",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/rcon/setup",
        tag: "rcon",
        summary: "开关 RCON",
        perm: "rcon.setup",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/rcon/exec",
        tag: "rcon",
        summary: "执行 RCON 命令",
        perm: "rcon",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/players",
        tag: "players",
        summary: "玩家列表",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/players/{name}/detail",
        tag: "players",
        summary: "玩家详情",
        perm: "players.detail",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/players/{name}/rcon",
        tag: "players",
        summary: "玩家 RCON 操作",
        perm: "rcon",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/players/{name}/{action}",
        tag: "players",
        summary: "玩家动作",
        perm: "players",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/backups",
        tag: "backups",
        summary: "备份列表",
        perm: "backups",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/backups",
        tag: "backups",
        summary: "创建备份",
        perm: "backups",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/backups/{bid}/restore",
        tag: "backups",
        summary: "恢复备份",
        perm: "backups",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/instances/{id}/backups/{bid}",
        tag: "backups",
        summary: "删除备份",
        perm: "backups",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/files",
        tag: "files",
        summary: "文件列表",
        perm: "files.read",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/files/content",
        tag: "files",
        summary: "读取文件",
        perm: "files.read",
        mutation: false,
    },
    Route {
        method: "put",
        path: "/api/v1/instances/{id}/files/content",
        tag: "files",
        summary: "写入文件",
        perm: "files.write",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/instances/{id}/files",
        tag: "files",
        summary: "删除路径",
        perm: "files.write",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/instances/{id}/plugins",
        tag: "plugins",
        summary: "插件列表",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/plugins/install",
        tag: "plugins",
        summary: "安装插件",
        perm: "plugins.install",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/instances/{id}/plugins/resolve",
        tag: "plugins",
        summary: "依赖解析",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/nodes",
        tag: "nodes",
        summary: "节点列表",
        perm: "nodes",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/nodes",
        tag: "nodes",
        summary: "创建节点",
        perm: "nodes.manage",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/nodes/{id}",
        tag: "nodes",
        summary: "删除节点",
        perm: "nodes.manage",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/nodes/{id}/drain",
        tag: "nodes",
        summary: "排空节点",
        perm: "nodes.manage",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/users",
        tag: "users",
        summary: "用户列表",
        perm: "users",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/users",
        tag: "users",
        summary: "创建用户",
        perm: "users",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/users/{id}",
        tag: "users",
        summary: "删除用户",
        perm: "users",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/roles",
        tag: "rbac",
        summary: "角色列表",
        perm: "users",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/roles",
        tag: "rbac",
        summary: "创建角色",
        perm: "roles.manage",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/grants",
        tag: "rbac",
        summary: "授予权限",
        perm: "roles.manage",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/grants/{id}",
        tag: "rbac",
        summary: "回收权限",
        perm: "roles.manage",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/apikeys",
        tag: "apikeys",
        summary: "API Key 列表",
        perm: "apikeys.manage",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/apikeys",
        tag: "apikeys",
        summary: "创建 API Key",
        perm: "apikeys.manage",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/apikeys/{id}",
        tag: "apikeys",
        summary: "撤销 API Key",
        perm: "apikeys.manage",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/sessions",
        tag: "sessions",
        summary: "登录设备",
        perm: "view",
        mutation: false,
    },
    Route {
        method: "delete",
        path: "/api/v1/sessions/{id}",
        tag: "sessions",
        summary: "踢下线设备",
        perm: "view",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/audit",
        tag: "audit",
        summary: "审计日志",
        perm: "audit.read",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/audit/verify",
        tag: "audit",
        summary: "审计链校验",
        perm: "audit.read",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/alerts",
        tag: "alerts",
        summary: "告警列表",
        perm: "metrics.read",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/alerts/{id}/silence",
        tag: "alerts",
        summary: "静默告警",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/workflows",
        tag: "workflows",
        summary: "工作流列表",
        perm: "automations",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/workflows/{id}/run",
        tag: "workflows",
        summary: "触发工作流",
        perm: "automations",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/tickets",
        tag: "tickets",
        summary: "工单列表",
        perm: "players",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/tickets/{id}/approve",
        tag: "tickets",
        summary: "通过工单",
        perm: "players",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/bans",
        tag: "bans",
        summary: "全局封禁列表",
        perm: "players",
        mutation: false,
    },
    Route {
        method: "post",
        path: "/api/v1/bans",
        tag: "bans",
        summary: "新增全局封禁",
        perm: "players.ban",
        mutation: true,
    },
    Route {
        method: "delete",
        path: "/api/v1/bans/{id}",
        tag: "bans",
        summary: "解除封禁",
        perm: "players.pardon",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/settings",
        tag: "settings",
        summary: "读取设置",
        perm: "settings",
        mutation: false,
    },
    Route {
        method: "put",
        path: "/api/v1/settings",
        tag: "settings",
        summary: "更新设置",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/storage",
        tag: "storage",
        summary: "存储配置",
        perm: "settings",
        mutation: false,
    },
    Route {
        method: "put",
        path: "/api/v1/storage",
        tag: "storage",
        summary: "更新存储配置",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "post",
        path: "/api/v1/storage/test",
        tag: "storage",
        summary: "测试存储连通性",
        perm: "settings",
        mutation: true,
    },
    Route {
        method: "get",
        path: "/api/v1/i18n/{lang}",
        tag: "system",
        summary: "语言包",
        perm: "public",
        mutation: false,
    },
    Route {
        method: "get",
        path: "/api/v1/openapi.json",
        tag: "system",
        summary: "接口文档",
        perm: "public",
        mutation: false,
    },
];

pub fn route_count() -> usize {
    ROUTES.len()
}

pub fn paths() -> Vec<&'static str> {
    ROUTES.iter().map(|r| r.path).collect()
}

pub fn tags() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in ROUTES {
        if !out.contains(&r.tag) {
            out.push(r.tag);
        }
    }
    out.sort();
    out
}

pub fn spec() -> Value {
    let mut paths = serde_json::Map::new();
    for route in ROUTES {
        let entry = paths
            .entry(route.path.to_string())
            .or_insert_with(|| json!({}));
        if let Some(map) = entry.as_object_mut() {
            let params: Vec<Value> = path_params(route.path)
                .into_iter()
                .map(|name| {
                    json!({
                        "name": name,
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    })
                })
                .collect();
            let mut op = json!({
                "tags": [route.tag],
                "summary": route.summary,
                "security": if route.perm == "public" { json!([]) } else { json!([{ "bearerAuth": [] }]) },
                "responses": {
                    "200": { "description": "成功" },
                    "401": { "description": "未认证" },
                    "403": { "description": "权限不足" },
                    "429": { "description": "请求过于频繁" }
                },
                "x-required-permission": route.perm,
                "x-mutation": route.mutation,
                "x-csrf-required": route.mutation && route.perm != "public"
            });
            if !params.is_empty() {
                op["parameters"] = Value::Array(params);
            }
            if route.mutation {
                op["requestBody"] = json!({
                    "required": true,
                    "content": { "application/json": { "schema": { "type": "object" } } }
                });
            }
            map.insert(route.method.to_string(), op);
        }
    }
    let tag_list: Vec<Value> = tags().into_iter().map(|t| json!({ "name": t })).collect();
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Cocktail Manager API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Minecraft 服务器控制面接口"
        },
        "servers": [{ "url": "/" }],
        "tags": tag_list,
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer" }
            },
            "schemas": {
                "Error": {
                    "type": "object",
                    "properties": {
                        "error": { "type": "string" },
                        "code": { "type": "string" }
                    }
                }
            }
        },
        "paths": Value::Object(paths)
    })
}

pub fn path_params(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end + 1..];
    }
    out
}

pub fn public_routes() -> Vec<&'static str> {
    ROUTES
        .iter()
        .filter(|r| r.perm == "public")
        .map(|r| r.path)
        .collect()
}

pub fn mutations() -> Vec<&'static str> {
    ROUTES
        .iter()
        .filter(|r| r.mutation)
        .map(|r| r.path)
        .collect()
}

pub fn permission_matrix() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut out: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    for route in ROUTES {
        if route.perm == "public" {
            continue;
        }
        match out.iter_mut().find(|(p, _)| *p == route.perm) {
            Some((_, list)) => list.push(route.path),
            None => out.push((route.perm, vec![route.path])),
        }
    }
    out.sort_by(|a, b| a.0.cmp(b.0));
    out
}

pub fn to_json_string() -> String {
    serde_json::to_string_pretty(&spec()).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_valid_openapi() {
        let s = spec();
        assert_eq!(s["openapi"], "3.0.3");
        assert!(s["info"]["title"].as_str().unwrap().contains("Cocktail"));
        assert!(s["paths"].is_object());
    }

    #[test]
    fn all_routes_present() {
        let s = spec();
        let paths = s["paths"].as_object().unwrap();
        assert!(paths.contains_key("/api/v1/instances"));
        assert!(paths.contains_key("/api/v1/instances/{id}/rcon/exec"));
        let rcon = paths["/api/v1/instances/{id}/rcon/exec"]
            .as_object()
            .unwrap();
        assert!(rcon.contains_key("post"));
    }

    #[test]
    fn path_params_extracted() {
        assert_eq!(path_params("/a/{id}/b/{name}"), vec!["id", "name"]);
        assert_eq!(path_params("/plain"), Vec::<&str>::new());
        assert_eq!(path_params("/x/{one}"), vec!["one"]);
    }

    #[test]
    fn params_emitted_for_templated_routes() {
        let s = spec();
        let op = &s["paths"]["/api/v1/instances/{id}/players/{name}/detail"]["get"];
        let params = op["parameters"].as_array().unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0]["name"], "id");
        assert_eq!(params[1]["name"], "name");
        assert_eq!(params[0]["in"], "path");
    }

    #[test]
    fn mutations_require_csrf_and_body() {
        let s = spec();
        let op = &s["paths"]["/api/v1/instances/{id}/rcon/exec"]["post"];
        assert_eq!(op["x-csrf-required"], true);
        assert!(op["requestBody"].is_object());
        assert_eq!(op["x-mutation"], true);
    }

    #[test]
    fn public_routes_have_no_security() {
        let s = spec();
        let op = &s["paths"]["/api/v1/health"]["get"];
        assert_eq!(op["security"], json!([]));
        assert_eq!(op["x-required-permission"], "public");
        let login = &s["paths"]["/api/v1/auth/login"]["post"];
        assert_eq!(login["x-csrf-required"], false);
    }

    #[test]
    fn permission_matrix_groups_by_perm() {
        let matrix = permission_matrix();
        assert!(matrix.iter().any(|(p, _)| *p == "players.ban"));
        let ban = matrix.iter().find(|(p, _)| *p == "players.ban").unwrap();
        assert!(ban.1.contains(&"/api/v1/bans"));
        assert!(matrix.iter().all(|(p, _)| *p != "public"));
    }

    #[test]
    fn public_and_mutation_lists_are_sane() {
        assert!(public_routes().contains(&"/api/v1/auth/login"));
        assert!(public_routes().contains(&"/api/v1/health"));
        assert!(mutations().contains(&"/api/v1/instances/{id}/start"));
        assert!(!mutations().contains(&"/api/v1/health"));
    }

    #[test]
    fn tags_are_deduplicated_and_sorted() {
        let t = tags();
        let mut sorted = t.clone();
        sorted.sort();
        assert_eq!(t, sorted);
        let unique: std::collections::BTreeSet<&str> = t.iter().copied().collect();
        assert_eq!(unique.len(), t.len());
    }

    #[test]
    fn json_string_is_parsable() {
        let text = to_json_string();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["openapi"], "3.0.3");
        assert!(route_count() > 70);
    }
}
