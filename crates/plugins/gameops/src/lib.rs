use cocktail_plugin_sdk::{
    FsReq, HttpReq, HttpResp, control, control_json, fs, json_body, log_info, log_warn, path_tail,
};
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const EXAMPLE: &str = r#"apiVersion: cocktail.gameops/v1
kind: World
metadata:
  name: survival
spec:
  folder: world
  retainSnapshots: 5
---
apiVersion: cocktail.gameops/v1
kind: PluginSet
metadata:
  name: survival-plugins
spec:
  group: survival
  items:
    - name: spark
      enabled: true
      source: local
---
apiVersion: cocktail.gameops/v1
kind: Proxy
metadata:
  name: hub
spec:
  instanceName: velocity-hub
  group: survival
  listenPort: 25577
  createIfMissing: true
  desiredRunning: true
---
apiVersion: cocktail.gameops/v1
kind: Network
metadata:
  name: smp
spec:
  worlds: [survival]
  pluginSet: survival-plugins
  proxy: hub
  desiredRunning: true
  servers:
    - name: smp-1
      nodeId: local
      core: paper
      memoryMib: 2048
      port: 25565
      world: survival
      group: survival
"#;

const OWNED: [&str; 4] = ["World", "PluginSet", "Proxy", "Network"];

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StoredObject {
    #[serde(default = "api_ver")]
    api_version: String,
    kind: String,
    name: String,
    #[serde(default)]
    labels: Value,
    spec: Value,
    #[serde(default)]
    status: Value,
    #[serde(default)]
    generation: u64,
}

fn api_ver() -> String {
    "cocktail.gameops/v1".into()
}

fn obj_key(kind: &str, name: &str) -> String {
    format!("objects/{}/{}", kind.to_ascii_lowercase(), name)
}

fn list_objects(kind: Option<&str>) -> Vec<StoredObject> {
    let kinds: Vec<&str> = kind.map(|k| vec![k]).unwrap_or_else(|| OWNED.to_vec());
    let mut out = Vec::new();
    for k in kinds {
        let listing = fs(&FsReq {
            op: "list".into(),
            path: format!("/data/kv/objects/{}", k.to_ascii_lowercase()),
            dest: None,
            body: None,
            body_b64: None,
        })
        .unwrap_or(cocktail_plugin_sdk::FsResp {
            ok: false,
            body: "[]".into(),
            error: String::new(),
            exists: false,
            size: 0,
        });
        let names: Vec<String> = serde_json::from_str(&listing.body).unwrap_or_default();
        for name in names {
            if let Some(obj) = get_object(k, name.trim_end_matches(".json")) {
                out.push(obj);
            }
        }
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    out
}

fn get_object(kind: &str, name: &str) -> Option<StoredObject> {
    let resp = fs(&FsReq {
        op: "read_text".into(),
        path: format!("/data/kv/{}", obj_key(kind, name)),
        dest: None,
        body: None,
        body_b64: None,
    })
    .ok()?;
    if !resp.exists && resp.body.is_empty() {
        return None;
    }
    serde_json::from_str(&resp.body).ok()
}

fn put_object(obj: &StoredObject) {
    let _ = fs(&FsReq {
        op: "write_text".into(),
        path: format!("/data/kv/{}", obj_key(&obj.kind, &obj.name)),
        dest: None,
        body: Some(serde_json::to_string_pretty(obj).unwrap_or_else(|_| "{}".into())),
        body_b64: None,
    });
}

fn upsert(
    kind: &str,
    name: &str,
    spec: Value,
    labels: Option<Value>,
) -> Result<StoredObject, String> {
    let kind = OWNED
        .iter()
        .find(|k| k.eq_ignore_ascii_case(kind))
        .copied()
        .ok_or_else(|| format!("kind {kind} is observed-only"))?;
    let existing = get_object(kind, name);
    let obj = StoredObject {
        api_version: api_ver(),
        kind: kind.into(),
        name: name.into(),
        labels: labels
            .or_else(|| existing.as_ref().map(|e| e.labels.clone()))
            .unwrap_or(json!({})),
        spec,
        status: existing
            .as_ref()
            .map(|e| e.status.clone())
            .unwrap_or(json!({ "message": "pending" })),
        generation: existing.map(|e| e.generation).unwrap_or(0) + 1,
    };
    put_object(&obj);
    Ok(obj)
}

fn put_status(kind: &str, name: &str, status: Value) {
    if let Some(mut obj) = get_object(kind, name) {
        obj.status = status;
        put_object(&obj);
    }
}

#[plugin_fn]
pub fn start(_: ()) -> FnResult<String> {
    log_info("GameOps kinds: World PluginSet Proxy Network");
    Ok("ok".into())
}

#[plugin_fn]
pub fn tick(_: ()) -> FnResult<String> {
    reconcile_all();
    Ok("ok".into())
}

#[plugin_fn]
pub fn http_handle(Json(req): Json<HttpReq>) -> FnResult<Json<HttpResp>> {
    let method = req.method.to_ascii_uppercase();
    let path = if req.path.trim_end_matches('/').is_empty() {
        "/".to_string()
    } else {
        req.path.trim_end_matches('/').to_string()
    };
    let resp = match (method.as_str(), path.as_str()) {
        ("GET", "/summary") => {
            let nodes: Value = control_json("GET", "/api/v1/nodes", None).unwrap_or(json!([]));
            let instances: Value =
                control_json("GET", "/api/v1/instances", None).unwrap_or(json!([]));
            HttpResp::ok_json(json!({
                "apiVersion": "cocktail.gameops/v1",
                "owned": OWNED,
                "observed": ["Node", "Instance"],
                "objects": list_objects(None),
                "nodes": nodes,
                "instances": instances,
            }))
        }
        ("GET", "/example") => HttpResp::text(200, EXAMPLE),
        ("GET", "/objects") => HttpResp::ok_json(json!({ "items": list_objects(None) })),
        ("POST", "/apply") => apply(&req),
        ("POST", "/reconcile") => {
            reconcile_all();
            HttpResp::ok_json(json!({ "ok": true, "items": list_objects(None) }))
        }
        _ if path.starts_with("/objects/") => objects_route(&method, &path, &req),
        _ if method == "POST" && path.starts_with("/worlds/") => {
            snapshot_world(path_tail(&path, "/worlds"))
        }
        _ => HttpResp::not_found(format!("no route {method} {path}")),
    };
    Ok(Json(resp))
}

fn apply(req: &HttpReq) -> HttpResp {
    let text = if let Ok(v) = json_body::<Value>(req) {
        v.get("yaml")
            .and_then(|x| x.as_str())
            .unwrap_or(&req.body)
            .to_string()
    } else {
        req.body.clone()
    };
    match parse_manifest(&text) {
        Ok(docs) => {
            let mut stored = Vec::new();
            for doc in docs {
                match upsert(&doc.kind, &doc.name, doc.spec, Some(doc.labels)) {
                    Ok(obj) => stored.push(obj),
                    Err(e) => return HttpResp::bad(e),
                }
            }
            reconcile_all();
            HttpResp::ok_json(json!({ "ok": true, "applied": stored.len(), "items": stored }))
        }
        Err(e) => HttpResp::bad(e),
    }
}

fn objects_route(method: &str, path: &str, req: &HttpReq) -> HttpResp {
    let rest = path_tail(path, "/objects");
    let mut parts = rest.split('/').filter(|s| !s.is_empty());
    let kind = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    match method {
        "GET" if name.is_empty() => HttpResp::ok_json(json!({ "items": list_objects(Some(kind)) })),
        "GET" => match get_object(kind, name) {
            Some(obj) => HttpResp::ok_json(obj),
            None => HttpResp::not_found("object not found"),
        },
        "PUT" if name.is_empty() => HttpResp::bad("PUT /objects/{kind}/{name}"),
        "PUT" => {
            let spec: Value = if req.body.is_empty() {
                json!({})
            } else {
                serde_json::from_str(&req.body).unwrap_or(json!({}))
            };
            let spec = spec.get("spec").cloned().unwrap_or(spec);
            match upsert(kind, name, spec, None) {
                Ok(obj) => HttpResp::ok_json(obj),
                Err(e) => HttpResp::bad(e),
            }
        }
        "DELETE" if name.is_empty() => HttpResp::bad("DELETE /objects/{kind}/{name}"),
        "DELETE" => {
            let _ = fs(&FsReq {
                op: "remove".into(),
                path: format!("/data/kv/{}", obj_key(kind, name)),
                dest: None,
                body: None,
                body_b64: None,
            });
            HttpResp::ok_json(json!({ "ok": true }))
        }
        _ => HttpResp::not_found("no route"),
    }
}

fn snapshot_world(name: &str) -> HttpResp {
    let name = name.split('/').next().unwrap_or(name);
    if name.is_empty() {
        return HttpResp::bad("world name required");
    }
    let Some(obj) = get_object("World", name) else {
        return HttpResp::bad("world not found");
    };
    let folder = obj
        .spec
        .get("folder")
        .and_then(|v| v.as_str())
        .unwrap_or("world");
    let retain = obj
        .spec
        .get("retainSnapshots")
        .and_then(|v| v.as_u64())
        .unwrap_or(5);
    let src = world_path(name, &obj.spec);
    let dest = format!("{src}/.snapshots/{}", now_stamp());
    let _ = fs(&FsReq {
        op: "copy_dir".into(),
        path: src.clone(),
        dest: Some(dest.clone()),
        body: None,
        body_b64: None,
    });
    let _ = folder;
    let _ = retain;
    HttpResp::ok_json(json!({ "ok": true, "snapshot": dest }))
}

fn parse_manifest(body: &str) -> Result<Vec<StoredObject>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return parse_json(trimmed);
    }
    let mut out = Vec::new();
    for chunk in trimmed.split("\n---") {
        let chunk = chunk.trim().trim_start_matches("---").trim();
        if chunk.is_empty() {
            continue;
        }
        let v: Value = serde_yaml::from_str(chunk).map_err(|e| e.to_string())?;
        out.push(from_element(&v)?);
    }
    Ok(out)
}

fn parse_json(json_text: &str) -> Result<Vec<StoredObject>, String> {
    let v: Value = serde_json::from_str(json_text).map_err(|e| e.to_string())?;
    if let Some(arr) = v.as_array() {
        arr.iter().map(from_element).collect()
    } else {
        Ok(vec![from_element(&v)?])
    }
}

fn from_element(el: &Value) -> Result<StoredObject, String> {
    let kind = el
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let meta = el.get("metadata").cloned().unwrap_or(json!({}));
    let name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| el.get("name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    if kind.is_empty() || name.is_empty() {
        return Err("manifest needs kind and metadata.name".into());
    }
    Ok(StoredObject {
        api_version: el
            .get("apiVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("cocktail.gameops/v1")
            .into(),
        kind,
        name,
        labels: meta.get("labels").cloned().unwrap_or(json!({})),
        spec: el.get("spec").cloned().unwrap_or(json!({})),
        status: json!({}),
        generation: 1,
    })
}

fn reconcile_all() {
    for obj in list_objects(None) {
        if let Err(e) = reconcile_one(&obj) {
            log_warn(format!("reconcile {}/{} failed: {e}", obj.kind, obj.name));
            put_status(
                &obj.kind,
                &obj.name,
                json!({ "ready": false, "message": e }),
            );
        }
    }
}

fn reconcile_one(obj: &StoredObject) -> Result<(), String> {
    match obj.kind.as_str() {
        "World" => reconcile_world(obj),
        "PluginSet" => reconcile_pluginset(obj),
        "Proxy" => reconcile_proxy(obj),
        "Network" => reconcile_network(obj),
        _ => Ok(()),
    }
}

fn world_path(name: &str, spec: &Value) -> String {
    spec.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("data/gameops/worlds/{name}"))
}

fn reconcile_world(obj: &StoredObject) -> Result<(), String> {
    let path = world_path(&obj.name, &obj.spec);
    let _ = fs(&FsReq {
        op: "mkdir".into(),
        path: path.clone(),
        dest: None,
        body: None,
        body_b64: None,
    });
    if let Some(clone) = obj.spec.get("cloneFrom").and_then(|v| v.as_str()) {
        let src = if clone.contains('/') {
            clone.to_string()
        } else {
            get_object("World", clone)
                .map(|o| world_path(&o.name, &o.spec))
                .unwrap_or_else(|| format!("data/gameops/worlds/{clone}"))
        };
        let _ = fs(&FsReq {
            op: "copy_dir".into(),
            path: src,
            dest: Some(path.clone()),
            body: None,
            body_b64: None,
        });
    }
    let size = fs(&FsReq {
        op: "dir_size".into(),
        path: path.clone(),
        dest: None,
        body: None,
        body_b64: None,
    })
    .map(|r| r.size)
    .unwrap_or(0);
    put_status(
        &obj.kind,
        &obj.name,
        json!({ "ready": true, "path": path, "bytes": size, "message": "world directory ready" }),
    );
    Ok(())
}

fn reconcile_pluginset(obj: &StoredObject) -> Result<(), String> {
    let instances: Vec<Value> = control_json("GET", "/api/v1/instances", None).unwrap_or_default();
    let group = obj.spec.get("group").and_then(|v| v.as_str());
    let names: Vec<String> = obj
        .spec
        .get("instanceNames")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let items = obj
        .spec
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut drift = Vec::new();
    for inst in instances {
        let spec = inst.get("spec").cloned().unwrap_or(json!({}));
        let inst_name = spec.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let inst_group = spec.get("group").and_then(|v| v.as_str()).unwrap_or("");
        let id = inst.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let matched = if !names.is_empty() {
            names.iter().any(|n| n.eq_ignore_ascii_case(inst_name))
        } else if let Some(g) = group {
            g.eq_ignore_ascii_case(inst_group)
        } else {
            false
        };
        if !matched {
            continue;
        }
        let have: Vec<Value> =
            control_json("GET", &format!("/api/v1/instances/{id}/plugins"), None)
                .unwrap_or_default();
        for item in &items {
            let want = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let enabled = item
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let source = item
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("local");
            let found = have.iter().find(|p| {
                let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                n.eq_ignore_ascii_case(want)
                    || n.trim_end_matches(".jar")
                        .eq_ignore_ascii_case(want.trim_end_matches(".jar"))
            });
            if found.is_none() {
                if source.eq_ignore_ascii_case("modrinth") {
                    if let Some(project) = item.get("project").and_then(|v| v.as_str()) {
                        let body = json!({ "project_id": project, "version_id": item.get("version"), "target": "plugin" }).to_string();
                        let _ = control(
                            "POST",
                            &format!("/api/v1/instances/{id}/modrinth/install"),
                            Some(&body),
                        );
                        drift.push(
                            json!({ "instanceId": id, "plugin": want, "action": "installed" }),
                        );
                        continue;
                    }
                }
                drift.push(json!({ "instanceId": id, "plugin": want, "want": "present", "have": "missing" }));
                continue;
            }
            if let Some(p) = found {
                let jar_on = p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                if jar_on != enabled {
                    let action = if enabled { "enable" } else { "disable" };
                    let _ = control(
                        "POST",
                        &format!(
                            "/api/v1/instances/{id}/plugins/{}/{action}",
                            p.get("name").and_then(|v| v.as_str()).unwrap_or(want)
                        ),
                        None,
                    );
                    drift.push(json!({ "instanceId": id, "plugin": want, "action": action }));
                }
            }
        }
    }
    put_status(
        &obj.kind,
        &obj.name,
        json!({ "ready": true, "drift": drift, "message": if drift.is_empty() { "in sync" } else { "drift recorded" } }),
    );
    Ok(())
}

fn reconcile_proxy(obj: &StoredObject) -> Result<(), String> {
    let mut instances: Vec<Value> =
        control_json("GET", "/api/v1/instances", None).unwrap_or_default();
    let name = obj
        .spec
        .get("instanceName")
        .and_then(|v| v.as_str())
        .unwrap_or(&obj.name)
        .to_string();
    let listen = obj
        .spec
        .get("listenPort")
        .and_then(|v| v.as_u64())
        .unwrap_or(25577);
    let group = obj.spec.get("group").and_then(|v| v.as_str()).unwrap_or("");
    let create = obj
        .spec
        .get("createIfMissing")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let desired = obj
        .spec
        .get("desiredRunning")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut proxy = instances
        .iter()
        .find(|i| {
            i.get("spec")
                .and_then(|s| s.get("name"))
                .and_then(|v| v.as_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(&name))
        })
        .cloned();
    if proxy.is_none() && create {
        let body = json!({
            "name": name,
            "core": "custom",
            "memory_mib": 512,
            "port": listen,
            "node_id": obj.spec.get("nodeId").and_then(|v| v.as_str()).unwrap_or("local"),
            "group": if group.is_empty() { "proxy" } else { group },
            "tags": ["proxy", "gameops"],
            "eula_accepted": true,
        })
        .to_string();
        if let Ok(created) = control_json::<Value>("POST", "/api/v1/instances", Some(&body)) {
            instances.push(created.clone());
            proxy = Some(created);
        }
    }
    let Some(proxy) = proxy else {
        put_status(
            &obj.kind,
            &obj.name,
            json!({ "ready": false, "message": format!("proxy instance {name} missing") }),
        );
        return Ok(());
    };
    let pid = proxy.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let motd = obj
        .spec
        .get("motd")
        .and_then(|v| v.as_str())
        .unwrap_or("Cocktail GameOps");
    let mut toml = format!("bind = \"0.0.0.0:{listen}\"\nmotd = \"{motd}\"\n[servers]\n");
    let mut backends = Vec::new();
    for inst in &instances {
        let id = inst.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id == pid {
            continue;
        }
        let spec = inst.get("spec").cloned().unwrap_or(json!({}));
        let ig = spec.get("group").and_then(|v| v.as_str()).unwrap_or("");
        if !group.is_empty() && !ig.eq_ignore_ascii_case(group) {
            continue;
        }
        let n = spec.get("name").and_then(|v| v.as_str()).unwrap_or(id);
        let port = spec.get("port").and_then(|v| v.as_u64()).unwrap_or(25565);
        let safe: String = n
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        toml.push_str(&format!("{safe} = \"127.0.0.1:{port}\"\n"));
        backends.push(json!({ "name": n, "instanceId": id }));
    }
    toml.push_str("[forced-hosts]\n");
    let body = json!({ "path": "velocity.toml", "content": toml }).to_string();
    let _ = control(
        "PUT",
        &format!("/api/v1/instances/{pid}/files/content"),
        Some(&body),
    );
    let st = proxy.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if desired && matches!(st, "stopped" | "created" | "crashed") {
        let _ = control("POST", &format!("/api/v1/instances/{pid}/start"), None);
    }
    put_status(
        &obj.kind,
        &obj.name,
        json!({ "ready": true, "instanceId": pid, "backends": backends, "message": "proxy backends synced" }),
    );
    Ok(())
}

fn reconcile_network(obj: &StoredObject) -> Result<(), String> {
    if let Some(worlds) = obj.spec.get("worlds").and_then(|v| v.as_array()) {
        for w in worlds {
            if let Some(name) = w.as_str() {
                if let Some(world) = get_object("World", name) {
                    let _ = reconcile_world(&world);
                }
            }
        }
    }
    let mut instances: Vec<Value> =
        control_json("GET", "/api/v1/instances", None).unwrap_or_default();
    let mut ids = Vec::new();
    let servers = obj
        .spec
        .get("servers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let desired = obj
        .spec
        .get("desiredRunning")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    for server in servers {
        let name = server.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let mut inst = instances
            .iter()
            .find(|i| {
                i.get("spec")
                    .and_then(|s| s.get("name"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .cloned();
        if inst.is_none() {
            let body = json!({
                "name": name,
                "core": server.get("core").and_then(|v| v.as_str()).unwrap_or("paper"),
                "memory_mib": server.get("memoryMib").and_then(|v| v.as_u64()).unwrap_or(2048),
                "port": server.get("port").and_then(|v| v.as_u64()).unwrap_or(25565),
                "node_id": server.get("nodeId").and_then(|v| v.as_str()).unwrap_or("local"),
                "group": server.get("group").and_then(|v| v.as_str()).unwrap_or("default"),
                "tags": ["gameops", obj.name],
                "eula_accepted": true,
                "auto_restart": true,
            })
            .to_string();
            if let Ok(created) = control_json::<Value>("POST", "/api/v1/instances", Some(&body)) {
                instances.push(created.clone());
                inst = Some(created);
            }
        }
        if let Some(inst) = inst {
            let id = inst
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            ids.push(id.clone());
            if let Some(world_name) = server.get("world").and_then(|v| v.as_str()) {
                if let Some(world) = get_object("World", world_name) {
                    attach_world(&inst, &world);
                }
            }
            let st = inst.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if desired && matches!(st, "stopped" | "created") {
                let _ = control("POST", &format!("/api/v1/instances/{id}/start"), None);
            }
        }
    }
    if let Some(set) = obj.spec.get("pluginSet").and_then(|v| v.as_str()) {
        if let Some(ps) = get_object("PluginSet", set) {
            let _ = reconcile_pluginset(&ps);
        }
    }
    if let Some(proxy) = obj.spec.get("proxy").and_then(|v| v.as_str()) {
        if let Some(p) = get_object("Proxy", proxy) {
            let _ = reconcile_proxy(&p);
        }
    }
    put_status(
        &obj.kind,
        &obj.name,
        json!({ "ready": true, "instanceIds": ids, "message": "network reconciled" }),
    );
    Ok(())
}

fn attach_world(inst: &Value, world: &StoredObject) {
    let spec = inst.get("spec").cloned().unwrap_or(json!({}));
    let workdir = spec.get("workdir").and_then(|v| v.as_str()).unwrap_or("");
    if workdir.is_empty() {
        return;
    }
    let folder = world
        .spec
        .get("folder")
        .and_then(|v| v.as_str())
        .unwrap_or("world");
    let src = world_path(&world.name, &world.spec);
    let dest = format!("{workdir}/{folder}");
    let _ = fs(&FsReq {
        op: "symlink".into(),
        path: src,
        dest: Some(dest),
        body: None,
        body_b64: None,
    });
}

fn now_stamp() -> String {
    format!("{}", (core::time::Duration::from_millis(1).as_millis()))
}
