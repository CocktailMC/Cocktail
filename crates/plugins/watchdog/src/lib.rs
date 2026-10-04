use cocktail_plugin_sdk::{
    HttpReq, HttpResp, control, json_body, kv_get_json, kv_set_json, log_info, log_warn,
};
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Config {
    auto_start_on_crash: bool,
    #[serde(default = "default_max")]
    max_incidents: usize,
}

fn default_max() -> usize {
    200
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Incident {
    instance_id: String,
    at: String,
    status: String,
    note: String,
}

fn cfg() -> Config {
    kv_get_json("config.json").unwrap_or_default()
}

fn incidents() -> Vec<Incident> {
    kv_get_json("incidents.json").unwrap_or_default()
}

fn save_incidents(mut items: Vec<Incident>, max: usize) {
    if items.len() > max {
        let drop_n = items.len() - max;
        items.drain(0..drop_n);
    }
    kv_set_json("incidents.json", &items);
}

#[plugin_fn]
pub fn start(_: ()) -> FnResult<String> {
    log_info("watchdog online");
    Ok("ok".into())
}

#[plugin_fn]
pub fn http_handle(Json(req): Json<HttpReq>) -> FnResult<Json<HttpResp>> {
    let method = req.method.to_ascii_uppercase();
    let path = req.path.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/".into() } else { path };
    let resp = match (method.as_str(), path.as_str()) {
        ("GET", "/summary") => {
            let c = cfg();
            let items = incidents();
            HttpResp::ok_json(json!({
                "plugin": "watchdog",
                "autoStartOnCrash": c.auto_start_on_crash,
                "total": items.len(),
                "incidents": items.iter().rev().take(50).cloned().collect::<Vec<_>>(),
            }))
        }
        ("POST", "/config") => {
            let patch: Config = match json_body(&req) {
                Ok(v) => v,
                Err(resp) => return Ok(Json(resp)),
            };
            let mut c = cfg();
            c.auto_start_on_crash = patch.auto_start_on_crash;
            if patch.max_incidents > 10 && patch.max_incidents < 5000 {
                c.max_incidents = patch.max_incidents;
            }
            kv_set_json("config.json", &c);
            HttpResp::ok_json(json!({
                "ok": true,
                "autoStartOnCrash": c.auto_start_on_crash,
                "max": c.max_incidents,
            }))
        }
        _ => HttpResp::not_found(format!("no route {method} {path}")),
    };
    Ok(Json(resp))
}

#[plugin_fn]
pub fn on_event(Json(ev): Json<Value>) -> FnResult<()> {
    let ty = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if ty != "status_changed" {
        return Ok(());
    }
    let status = ev.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if !status.eq_ignore_ascii_case("crashed") {
        return Ok(());
    }
    let instance_id = ev
        .get("instance_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let at = ev
        .get("at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut items = incidents();
    items.push(Incident {
        instance_id: instance_id.clone(),
        at,
        status: status.into(),
        note: "status crashed".into(),
    });
    let c = cfg();
    save_incidents(items, c.max_incidents);
    log_warn(format!("crash recorded for {instance_id}"));
    if c.auto_start_on_crash {
        match control(
            "POST",
            &format!("/api/v1/instances/{instance_id}/start"),
            None,
        ) {
            Ok(resp) if resp.ok => log_info(format!("requested start after crash: {instance_id}")),
            Ok(resp) => log_warn(format!("auto-start failed: {}", resp.body)),
            Err(e) => log_warn(format!("auto-start failed: {e}")),
        }
    }
    Ok(())
}

#[plugin_fn]
pub fn tick(_: ()) -> FnResult<String> {
    let c = cfg();
    save_incidents(incidents(), c.max_incidents);
    Ok("ok".into())
}
