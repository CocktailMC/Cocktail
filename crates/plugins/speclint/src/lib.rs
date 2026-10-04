use cocktail_plugin_sdk::{HttpReq, HttpResp, control_json, log_info};
use extism_pdk::*;
use serde_json::{Value, json};

#[plugin_fn]
pub fn start(_: ()) -> FnResult<String> {
    log_info("spec lint plugin online");
    Ok("ok".into())
}

#[plugin_fn]
pub fn http_handle(Json(req): Json<HttpReq>) -> FnResult<Json<HttpResp>> {
    let path = req.path.trim_end_matches('/');
    if req.method.to_ascii_uppercase() != "GET" || (path != "/report" && path != "") {
        return Ok(Json(HttpResp::not_found("no route")));
    }
    let instances: Vec<Value> = control_json("GET", "/api/v1/instances", None).unwrap_or_default();
    let mut findings = Vec::new();
    let mut used_ports: Vec<(i64, String)> = Vec::new();
    for inst in &instances {
        let spec = inst.get("spec").cloned().unwrap_or(json!({}));
        let name = spec
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = inst.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let status = inst.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let core = spec.get("core").and_then(|v| v.as_str()).unwrap_or("");
        let eula = spec
            .get("eula_accepted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if core != "demo" && !eula {
            findings.push(finding(
                id,
                &name,
                inst,
                "eula",
                "error",
                "未同意 EULA，无法作为可玩状态上线",
            ));
        }
        let desired = spec
            .get("desired_running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || inst
                .get("desired_running")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        if desired && matches!(status, "stopped" | "created" | "crashed") {
            findings.push(finding(
                id,
                &name,
                inst,
                "drift",
                "warn",
                &format!("期望运行但现状为 {status}"),
            ));
        }
        let port = spec.get("port").and_then(|v| v.as_i64()).unwrap_or(0);
        if !(1..=65535).contains(&port) {
            findings.push(finding(
                id,
                &name,
                inst,
                "port",
                "error",
                &format!("非法端口 {port}"),
            ));
        } else if let Some((_, other)) = used_ports.iter().find(|(p, _)| *p == port) {
            findings.push(finding(
                id,
                &name,
                inst,
                "port",
                "warn",
                &format!("端口 {port} 与 {other} 冲突（同清单）"),
            ));
        } else {
            used_ports.push((port, name.clone()));
        }
        let node = inst
            .get("node_id")
            .and_then(|v| v.as_str())
            .or_else(|| spec.get("node_id").and_then(|v| v.as_str()))
            .unwrap_or("");
        if node.is_empty() {
            findings.push(finding(id, &name, inst, "node", "warn", "未指定 node_id"));
        }
    }
    Ok(Json(HttpResp::ok_json(json!({
        "plugin": "speclint",
        "instances": instances.len(),
        "findings": findings,
    }))))
}

fn finding(id: &str, name: &str, inst: &Value, code: &str, severity: &str, message: &str) -> Value {
    json!({
        "instanceId": id,
        "name": name,
        "code": code,
        "severity": severity,
        "message": message,
        "generation": inst.get("generation"),
    })
}
