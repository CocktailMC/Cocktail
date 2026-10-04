use cocktail_plugin_sdk::{
    HostHttpReq, HttpReq, HttpResp, b64_decode, control, control_json, control_upload, http,
    json_body, kv_get_json, kv_set_json, log_info, path_tail,
};
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CONFIG_PATH: &str = "config/esplus-common.toml";
const DEFAULT_USER: &str = "admin";
const DEFAULT_PW: &str = "esplus";

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdapterConfig {
    #[serde(default = "default_repo")]
    github_repo: String,
    jar_path: Option<String>,
    #[serde(default)]
    instances: std::collections::BTreeMap<String, InstanceSecrets>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            github_repo: default_repo(),
            jar_path: None,
            instances: Default::default(),
        }
    }
}

fn default_repo() -> String {
    "FORGE24/ESPlus".into()
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct InstanceSecrets {
    username: Option<String>,
    password: Option<String>,
    password_set: bool,
    panel_port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstanceAction {
    instance_id: String,
    username: Option<String>,
    password: Option<String>,
    panel_port: Option<u16>,
    panel_enabled: Option<bool>,
}

fn cfg() -> AdapterConfig {
    kv_get_json("config.json").unwrap_or_default()
}

fn save_cfg(c: &AdapterConfig) {
    kv_set_json("config.json", c);
}

#[plugin_fn]
pub fn start(_: ()) -> FnResult<String> {
    let c = cfg();
    log_info(format!("ESPlus adapter online (repo={})", c.github_repo));
    Ok("ok".into())
}

#[plugin_fn]
pub fn http_handle(Json(req): Json<HttpReq>) -> FnResult<Json<HttpResp>> {
    let method = req.method.to_ascii_uppercase();
    let path = req.path.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/".into() } else { path };
    let resp = match (method.as_str(), path.as_str()) {
        ("GET", "/summary") => summary(),
        ("GET", "/example") => HttpResp::text(200, include_str!("example.txt")),
        ("POST", "/config") => {
            let patch: Value = json_body(&req).unwrap_or(json!({}));
            let mut c = cfg();
            if let Some(repo) = patch.get("githubRepo").and_then(|v| v.as_str()) {
                c.github_repo = repo.trim().into();
            }
            if let Some(p) = patch.get("jarPath").and_then(|v| v.as_str()) {
                c.jar_path = Some(p.into());
            }
            save_cfg(&c);
            HttpResp::ok_json(json!({ "ok": true, "githubRepo": c.github_repo }))
        }
        ("POST", "/ensure-config") => match json_body::<InstanceAction>(&req) {
            Ok(body) if !body.instance_id.is_empty() => ensure_config(&body),
            _ => HttpResp::bad("instanceId required"),
        },
        ("POST", "/install") => match json_body::<InstanceAction>(&req) {
            Ok(body) if !body.instance_id.is_empty() => install(&body),
            _ => HttpResp::bad("instanceId required"),
        },
        ("GET", p) if p.starts_with("/panel/") => panel_proxy(path_tail(p, "/panel")),
        _ => HttpResp::not_found(format!("no route {method} {path}")),
    };
    Ok(Json(resp))
}

fn summary() -> HttpResp {
    let c = cfg();
    let instances: Vec<Value> = control_json("GET", "/api/v1/instances", None).unwrap_or_default();
    let mut rows = Vec::new();
    for inst in instances {
        let id = inst
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let spec = inst.get("spec").cloned().unwrap_or(json!({}));
        let secrets = c.instances.get(&id).cloned().unwrap_or_default();
        let files: Vec<Value> = control_json(
            "GET",
            &format!("/api/v1/instances/{id}/files?path=mods"),
            None,
        )
        .unwrap_or_default();
        let jar = files.iter().find_map(|e| {
            let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let dir = e.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            if !dir && is_mod_jar(name) {
                Some(name.to_string())
            } else {
                None
            }
        });
        let toml = read_file(&id, CONFIG_PATH);
        let port = secrets.panel_port.unwrap_or_else(|| {
            toml.as_ref()
                .map(|t| read_int(t, "panelPort", 8088))
                .unwrap_or(8088)
        });
        let bind = toml
            .as_ref()
            .map(|t| read_string(t, "panelBindAddress", "127.0.0.1"))
            .unwrap_or_else(|| "127.0.0.1".into());
        let password = toml
            .as_ref()
            .map(|t| read_string(t, "panelPassword", ""))
            .unwrap_or_default();
        let default_pw = password == DEFAULT_PW;
        let status = inst.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let mut reachable = None;
        if jar.is_some() && matches!(status, "running" | "online") {
            reachable = Some(probe(&format!("http://{bind}:{port}/")));
        }
        rows.push(json!({
            "id": id,
            "name": spec.get("name"),
            "status": status,
            "core": spec.get("core"),
            "installed": jar.is_some(),
            "jar": jar,
            "panelUrl": format!("http://{bind}:{port}/"),
            "panelReachable": reachable,
            "passwordIsDefault": default_pw,
        }));
    }
    HttpResp::ok_json(json!({
        "plugin": "esplus",
        "source": "https://github.com/FORGE24/ESPlus",
        "githubRepo": c.github_repo,
        "instances": rows,
    }))
}

fn ensure_config(body: &InstanceAction) -> HttpResp {
    let mut c = cfg();
    let used: Vec<u16> = c
        .instances
        .iter()
        .filter(|(k, _)| *k != &body.instance_id)
        .filter_map(|(_, s)| s.panel_port)
        .collect();
    let toml = read_file(&body.instance_id, CONFIG_PATH);
    let secrets = c.instances.entry(body.instance_id.clone()).or_default();
    let port = body.panel_port.or(secrets.panel_port).unwrap_or_else(|| {
        toml.as_ref()
            .map(|t| read_int(t, "panelPort", 8088))
            .unwrap_or_else(|| suggest_port(&body.instance_id, &used))
    });
    let username = body
        .username
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| secrets.username.clone())
        .unwrap_or_else(|| DEFAULT_USER.into());
    let mut generated = false;
    let mut password = body
        .password
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| secrets.password.clone())
        .unwrap_or_default();
    if password.is_empty() || password == DEFAULT_PW {
        password = format!("{:x}", hash_id(&format!("{}-pw", body.instance_id)));
        generated = true;
    }
    let next = apply_panel(
        toml.as_deref().unwrap_or(""),
        port,
        "127.0.0.1",
        &username,
        &password,
        body.panel_enabled.unwrap_or(true),
    );
    let _ = control(
        "POST",
        &format!("/api/v1/instances/{}/files/mkdir", body.instance_id),
        Some(&json!({ "path": "config" }).to_string()),
    );
    let write = json!({ "path": CONFIG_PATH, "content": next }).to_string();
    if let Err(e) = control(
        "PUT",
        &format!("/api/v1/instances/{}/files/content", body.instance_id),
        Some(&write),
    ) {
        return HttpResp::bad(e);
    }
    secrets.username = Some(username.clone());
    secrets.password = Some(password.clone());
    secrets.password_set = true;
    secrets.panel_port = Some(port);
    save_cfg(&c);
    HttpResp::ok_json(json!({
        "ok": true,
        "instanceId": body.instance_id,
        "panelPort": port,
        "panelBind": "127.0.0.1",
        "panelUsername": username,
        "passwordGenerated": generated,
        "panelPassword": if generated { Some(password) } else { None },
        "hint": if generated {
            "panelPassword was generated and written to config/esplus-common.toml; restart the instance to apply."
        } else {
            "panel config updated; restart the instance if it is already running."
        }
    }))
}

fn install(body: &InstanceAction) -> HttpResp {
    match resolve_jar() {
        Ok((bytes, file_name)) => {
            let _ = control(
                "POST",
                &format!("/api/v1/instances/{}/files/mkdir", body.instance_id),
                Some(&json!({ "path": "mods" }).to_string()),
            );
            if let Err(e) = control_upload(
                &body.instance_id,
                &format!("mods/{file_name}"),
                &file_name,
                &bytes,
            ) {
                return HttpResp::bad(e);
            }
            let cfg = ensure_config(body);
            HttpResp::ok_json(
                json!({ "ok": true, "jar": file_name, "config": serde_json::from_str::<Value>(&cfg.body).ok() }),
            )
        }
        Err(e) => HttpResp::bad(e),
    }
}

fn resolve_jar() -> Result<(Vec<u8>, String), String> {
    let c = cfg();
    let repo = c.github_repo;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let latest = http(&HostHttpReq {
        url,
        method: "GET".into(),
        headers: vec![
            ("User-Agent".into(), "Cocktail-ESPlus-Adapter/0.1".into()),
            ("Accept".into(), "application/vnd.github+json".into()),
        ],
        body: None,
        form: None,
    })?;
    let root: Value = if latest.status < 400 {
        serde_json::from_str(&latest.body).unwrap_or(json!({}))
    } else {
        let list = http(&HostHttpReq {
            url: format!("https://api.github.com/repos/{repo}/releases?per_page=5"),
            method: "GET".into(),
            headers: vec![("User-Agent".into(), "Cocktail-ESPlus-Adapter/0.1".into())],
            body: None,
            form: None,
        })?;
        let arr: Value = serde_json::from_str(&list.body).unwrap_or(json!([]));
        arr.as_array()
            .and_then(|a| a.first().cloned())
            .ok_or_else(|| format!("no GitHub releases on {repo}; set a local jar"))?
    };
    let assets = root
        .get("assets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(is_mod_jar)
        })
        .ok_or_else(|| "release has no esplus-*.jar asset".to_string())?;
    let download = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing download url".to_string())?;
    let file_name = asset
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("esplus.jar")
        .to_string();
    let jar = http(&HostHttpReq {
        url: download.into(),
        method: "GET".into(),
        headers: vec![("User-Agent".into(), "Cocktail-ESPlus-Adapter/0.1".into())],
        body: None,
        form: None,
    })?;
    if jar.status >= 400 {
        return Err(format!("download failed HTTP {}", jar.status));
    }
    let bytes = if jar.body_b64.is_empty() {
        jar.body.into_bytes()
    } else {
        b64_decode(&jar.body_b64)?
    };
    Ok((bytes, file_name))
}

fn panel_proxy(rest: &str) -> HttpResp {
    let mut parts = rest.split('/');
    let id = parts.next().unwrap_or("");
    let kind = parts.next().unwrap_or("dashboard");
    if id.is_empty() {
        return HttpResp::bad("instance id required");
    }
    let allowed = matches!(kind, "dashboard" | "alerts" | "audit" | "players");
    if !allowed {
        return HttpResp::bad("kind must be dashboard, alerts, audit, or players");
    }
    let c = cfg();
    let secrets = c.instances.get(id).cloned().unwrap_or_default();
    let toml = read_file(id, CONFIG_PATH);
    let port = secrets.panel_port.unwrap_or_else(|| {
        toml.as_ref()
            .map(|t| read_int(t, "panelPort", 8088))
            .unwrap_or(8088)
    });
    let bind = toml
        .as_ref()
        .map(|t| read_string(t, "panelBindAddress", "127.0.0.1"))
        .unwrap_or_else(|| "127.0.0.1".into());
    let username = secrets
        .username
        .or_else(|| {
            toml.as_ref()
                .map(|t| read_string(t, "panelUsername", DEFAULT_USER))
        })
        .unwrap_or_else(|| DEFAULT_USER.into());
    let password = secrets
        .password
        .or_else(|| toml.as_ref().map(|t| read_string(t, "panelPassword", "")))
        .unwrap_or_default();
    if password.is_empty() {
        return HttpResp::bad("no panel password stored; run ensure-config first");
    }
    let base = format!("http://{bind}:{port}/");
    if !probe(&base) {
        return HttpResp::json(502, json!({ "error": "panel unreachable or login failed" }));
    }
    let login = http(&HostHttpReq {
        url: format!("{base}api/auth/login"),
        method: "POST".into(),
        headers: vec![],
        body: None,
        form: Some(vec![
            ("username".into(), username),
            ("password".into(), password),
        ]),
    });
    match login {
        Ok(resp) if resp.status < 400 => {
            let path = match kind {
                "alerts" => "api/alerts",
                "audit" => "api/audit",
                "players" => "api/players/online",
                _ => "api/dashboard",
            };
            let cookie = resp
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, v)| v.clone());
            let mut headers = vec![];
            if let Some(c) = cookie {
                headers.push(("Cookie".into(), c));
            }
            match http(&HostHttpReq {
                url: format!("{base}{path}"),
                method: "GET".into(),
                headers,
                body: None,
                form: None,
            }) {
                Ok(page) => HttpResp {
                    status: page.status,
                    content_type: "application/json".into(),
                    body: page.body,
                },
                Err(e) => HttpResp::bad(e),
            }
        }
        Ok(resp) => HttpResp::bad(format!("login HTTP {}", resp.status)),
        Err(e) => HttpResp::bad(e),
    }
}

fn probe(base: &str) -> bool {
    http(&HostHttpReq {
        url: format!("{base}login"),
        method: "GET".into(),
        headers: vec![],
        body: None,
        form: None,
    })
    .ok()
    .is_some_and(|r| r.status < 500)
}

fn read_file(id: &str, path: &str) -> Option<String> {
    let resp = control(
        "GET",
        &format!(
            "/api/v1/instances/{id}/files/content?path={}",
            path.replace(' ', "%20")
        ),
        None,
    )
    .ok()?;
    if !resp.ok {
        return None;
    }
    let v: Value = serde_json::from_str(&resp.body).ok()?;
    v.get("content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn is_mod_jar(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("esplus")
        && n.ends_with(".jar")
        && !n.contains("sources")
        && !n.contains("javadoc")
        && !n.contains("panel")
}

fn read_int(toml: &str, key: &str, fallback: u16) -> u16 {
    for line in toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim().trim_start_matches('=').trim();
            if let Ok(n) = rest.parse() {
                return n;
            }
        }
    }
    fallback
}

fn read_string(toml: &str, key: &str, fallback: &str) -> String {
    for line in toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim().trim_start_matches('=').trim().trim_matches('"');
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    fallback.into()
}

fn upsert_line(toml: &str, key: &str, rendered: &str) -> String {
    let mut found = false;
    let mut out = String::new();
    for line in toml.lines() {
        if line.trim_start().starts_with(&format!("{key} "))
            || line.trim_start().starts_with(&format!("{key}="))
        {
            out.push_str(rendered);
            out.push('\n');
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        if out.is_empty() {
            out.push_str("# Cocktail ESPlus adapter\n");
        }
        out.push_str(rendered);
        out.push('\n');
    }
    out
}

fn apply_panel(
    toml: &str,
    port: u16,
    bind: &str,
    user: &str,
    password: &str,
    enabled: bool,
) -> String {
    let mut next = if toml.trim().is_empty() {
        "# Written by Cocktail ESPlus adapter.\n".into()
    } else {
        toml.to_string()
    };
    next = upsert_line(
        &next,
        "panelEnabled",
        &format!("panelEnabled = {}", if enabled { "true" } else { "false" }),
    );
    next = upsert_line(&next, "panelPort", &format!("panelPort = {port}"));
    next = upsert_line(
        &next,
        "panelBindAddress",
        &format!("panelBindAddress = \"{bind}\""),
    );
    next = upsert_line(
        &next,
        "panelUsername",
        &format!("panelUsername = \"{user}\""),
    );
    next = upsert_line(
        &next,
        "panelPassword",
        &format!("panelPassword = \"{password}\""),
    );
    upsert_line(
        &next,
        "panelAllowDefaultPassword",
        "panelAllowDefaultPassword = false",
    )
}

fn suggest_port(instance_id: &str, used: &[u16]) -> u16 {
    let seed = hash_id(instance_id) % 120;
    for i in 0..120u32 {
        let port = 8088 + ((seed + i) % 120) as u16;
        if !used.contains(&port) {
            return port;
        }
    }
    8088
}

fn hash_id(s: &str) -> u32 {
    let mut h = 2166136261u32;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}
