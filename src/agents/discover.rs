//! discover.rs - 服务 API 发现 + OpenAPI->KB 转换。
//!
//! docker-agent 部署容器后,调 discover_service 探测 OpenAPI,
//! openapi_to_kb 转成 ServiceKb,写 kb-<service>.json + 注册 agent(动态纳管)。
//!
//! 三档发现:① 模板库(已知服务,后续)② OpenAPI 自动(本模块)③ LLM 读文档(兜底,后续)。

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

use crate::agents::generic::{KbAction, KbParam, ServiceKb};
use crate::models::ModelLibrary;

/// 常见 OpenAPI 文档路径,按优先级试。
const OPENAPI_PATHS: &[&str] = &[
    "/openapi.json",
    "/swagger.json",
    "/v3/api-docs",
    "/api/openapi.json",
    "/api/v1/openapi.json",
];

/// 跳过这些前缀的端点(内部/无关/管理类)。
const SKIP_PREFIXES: &[&str] = &[
    "/auth", "/login", "/logout", "/health", "/heartbeat", "/metrics", "/docs", "/openapi",
    "/swagger", "/api/docs", "/favicon",
];

/// 探测服务的 OpenAPI,返回 spec。逐个试常见路径,命中含 paths 的即返回。
pub async fn discover_service(base_url: &str, token: Option<&str>) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut last_err = String::new();
    for path in OPENAPI_PATHS {
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let mut req = client.get(&url);
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("{path} -> {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            last_err = format!("{path} -> {}", resp.status());
            continue;
        }
        // 容错:非 JSON(如 HTML 前端)跳过该路径,不 bail
        let body: Value = match resp.json().await {
            Ok(b) => b,
            Err(_) => {
                last_err = format!("{path}: 非 JSON 响应");
                continue;
            }
        };
        if body.get("paths").is_some() {
            tracing::info!("发现 OpenAPI: {url}");
            return Ok(body);
        }
        last_err = format!("{path}: 无 paths 字段");
    }
    anyhow::bail!("未发现 OpenAPI(试了 {:?}): {}", OPENAPI_PATHS, last_err)
}

/// OpenAPI spec -> ServiceKb。遍历 paths×methods 生成 actions。
/// auth 不自动发现(OpenAPI securitySchemes 复杂),留 None,用户经 Key 管理配。
pub fn openapi_to_kb(spec: &Value, service_name: &str, base_url_env: &str, source: &str) -> Result<ServiceKb> {
    let paths = spec
        .get("paths")
        .and_then(|p| p.as_object())
        .context("OpenAPI 无 paths")?;
    let mut actions: HashMap<String, KbAction> = HashMap::new();
    for (path, item) in paths {
        if SKIP_PREFIXES.iter().any(|p| path.starts_with(p)) {
            continue;
        }
        let item = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        for method in &["get", "post", "put", "patch", "delete"] {
            if let Some(op) = item.get(*method) {
                if let Some((name, action)) = op_to_action(method, path, op) {
                    actions.insert(name, action);
                }
            }
        }
    }
    if actions.is_empty() {
        anyhow::bail!("OpenAPI 无可用端点(全被过滤或为空)");
    }
    tracing::info!("openapi_to_kb: {} 生成 {} 个 action", service_name, actions.len());
    Ok(ServiceKb {
        domain: service_name.to_string(),
        source: source.to_string(),
        base_url_env: base_url_env.to_string(),
        auth: None,
        actions,
    })
}

/// 单个 operation -> (action名, KbAction)。名优先 operationId,否则 method+path 合成。
fn op_to_action(method: &str, path: &str, op: &Value) -> Option<(String, KbAction)> {
    let description = op
        .get("summary")
        .and_then(|s| s.as_str())
        .or_else(|| op.get("description").and_then(|d| d.as_str()))
        .unwrap_or("")
        .to_string();
    let name = op
        .get("operationId")
        .and_then(|o| o.as_str())
        .map(slug)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| synthetic_name(method, path));
    if name.is_empty() {
        return None;
    }
    let params = op
        .get("parameters")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(param_to_kbparam).collect())
        .unwrap_or_default();
    let class = match method {
        "get" => "read",
        "delete" => "destructive",
        _ => "write",
    }
    .to_string();
    Some((
        name,
        KbAction {
            method: method.to_uppercase(),
            path: path.to_string(),
            description,
            class,
            params,
            response_path: String::new(),
        },
    ))
}

/// OpenAPI parameter -> KbParam。type 从 schema.type 取。
fn param_to_kbparam(p: &Value) -> Option<KbParam> {
    let name = p.get("name")?.as_str()?.to_string();
    let location = p.get("in").and_then(|x| x.as_str()).unwrap_or("query").to_string();
    let required = p.get("required").and_then(|r| r.as_bool()).unwrap_or(false);
    let param_type = p
        .get("schema")
        .and_then(|s| s.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("string")
        .to_string();
    Some(KbParam { name, location, param_type, required })
}

/// operationId -> slug(小写,非字母数字转 _)。
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// 无 operationId 时:method + 路径最后一段(跳过 {param} 段)。
fn synthetic_name(method: &str, path: &str) -> String {
    let segs: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .collect();
    format!("{}_{}", method, segs.last().unwrap_or(&"root"))
}

/// 把 ServiceKb 写成 kb-<name>.json 到 /etc/aaos/(回退当前目录)。
pub fn write_kb(kb: &ServiceKb) -> Result<String> {
    let json = serde_json::to_string_pretty(kb)?;
    let path = std::fs::canonicalize("/etc/aaos/")
        .map(|_| format!("/etc/aaos/kb-{}.json", kb.domain))
        .unwrap_or_else(|_| format!("kb-{}.json", kb.domain));
    std::fs::write(&path, &json).with_context(|| format!("写 {path}"))?;
    Ok(path)
}

/// 往 agents.json 注册动态 agent(读-改-写,已存在则更新)。
pub fn register_agent(name: &str, description: &str, kb: &ServiceKb) -> Result<()> {
    let path = std::fs::canonicalize("/etc/aaos/")
        .map(|_| "/etc/aaos/agents.json".to_string())
        .unwrap_or_else(|_| "agents.json".to_string());
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "[]".into());
    let mut arr: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
    let actions: Vec<Value> = kb
        .actions
        .iter()
        .map(|(n, a)| {
            json!({
                "name": n, "description": a.description,
                "params": a.params.iter().map(|p| json!({"name": p.name, "type": p.param_type, "required": p.required})).collect::<Vec<_>>()
            })
        })
        .collect();
    let entry = json!({"name": name, "description": description, "type": "http", "kb": format!("kb-{}.json", name), "actions": actions});
    if let Some(pos) = arr.iter().position(|a| a.get("name").and_then(|n| n.as_str()) == Some(name)) {
        arr[pos] = entry;
    } else {
        arr.push(entry);
    }
    std::fs::write(&path, serde_json::to_string_pretty(&arr)?)?;
    tracing::info!("注册动态 agent: {name} -> {path}");
    Ok(())
}

// ===== DRF OPTIONS 发现(Django/DRF 项目,无 openapi 时回退)=====

/// DRF 根解析:{"<route>":"<url>"} -> 路由列表。自定义 DRF(MusicTag {result,code,message})返回空。
pub fn parse_drf_root(root: &Value) -> Vec<(String, String)> {
    root.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    // DRF 根 value 是 URL(http... 或 /api...);过滤非 URL(如 MusicTag {code:"40101"})
                    v.as_str().filter(|s| s.starts_with("http") || s.starts_with('/')).map(|s| (k.clone(), s.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// DRF OPTIONS meta -> actions:GET list(read)+ POST create(write,fields 来自 actions.POST.fields)。
pub fn drf_meta_to_actions(rname: &str, path: &str, meta: &Value) -> Vec<(String, KbAction)> {
    let mut out = vec![(
        format!("list_{rname}"),
        KbAction { method: "GET".into(), path: path.into(), description: format!("List {rname}"), class: "read".into(), params: vec![], response_path: String::new() },
    )];
    if let Some(fields) = meta.get("actions").and_then(|a| a.get("POST")).and_then(|p| p.get("fields")).and_then(|f| f.as_object()) {
        let params: Vec<KbParam> = fields
            .iter()
            .map(|(fname, fmeta)| KbParam {
                name: fname.clone(),
                location: "body".into(),
                param_type: fmeta.get("type").and_then(|t| t.as_str()).unwrap_or("string").into(),
                required: fmeta.get("required").and_then(|r| r.as_bool()).unwrap_or(false),
            })
            .collect();
        out.push((format!("create_{rname}"), KbAction { method: "POST".into(), path: path.into(), description: format!("Create {rname}"), class: "write".into(), params, response_path: String::new() }));
    }
    out
}

/// DRF 发现:GET /api/ 根 -> 每个路由 OPTIONS -> ServiceKb。覆盖标准 DRF(MusicTag 自定义 DRF 不适用)。
pub async fn discover_drf(base_url: &str, token: Option<&str>) -> Result<ServiceKb> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;
    let root_url = format!("{}/api/", base_url.trim_end_matches('/'));
    let root = authed_req(&client, reqwest::Method::GET, &root_url, token).await
        .context("GET /api/ DRF 根失败(非 DRF 或需鉴权)")?;
    let routes = parse_drf_root(&root);
    if routes.is_empty() {
        anyhow::bail!("DRF 根无子路由(非标准 DRF,如 MusicTag 自定义响应)");
    }
    let mut actions = HashMap::new();
    for (rname, url) in &routes {
        let path = url_path(base_url, url);
        let meta = match authed_req(&client, reqwest::Method::OPTIONS, url, token).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        for (name, act) in drf_meta_to_actions(rname, &path, &meta) {
            actions.insert(name, act);
        }
    }
    if actions.is_empty() {
        anyhow::bail!("DRF OPTIONS 未解析出 action");
    }
    Ok(ServiceKb { domain: String::new(), source: String::new(), base_url_env: String::new(), auth: None, actions })
}

/// 带可选 token 的请求,返回 JSON。
async fn authed_req(client: &reqwest::Client, method: reqwest::Method, url: &str, token: Option<&str>) -> Result<Value> {
    let mut req = client.request(method, url);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send().await.context("请求失败")?;
    if !resp.status().is_success() {
        anyhow::bail!("{} 返回 {}", url, resp.status());
    }
    resp.json().await.context("解析 JSON 失败")
}

/// 从完整 url 提取 path 部分(去掉 base_url 前缀)。
fn url_path(base_url: &str, url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    url.strip_prefix(base).map(|p| p.to_string()).unwrap_or_else(|| url.to_string())
}

/// 完整流程:发现 OpenAPI -> 转 KB -> 写文件 -> 注册 agent。返回 (action 数, kb 路径)。
/// openapi 失败时回退到 DRF OPTIONS 发现(Django/DRF 项目)。
pub async fn discover_and_register(name: &str, base_url: &str, token: Option<&str>, base_url_env: &str, source: &str, description: &str) -> Result<(usize, String)> {
    let mut kb = match discover_service(base_url, token).await {
        Ok(spec) => openapi_to_kb(&spec, name, base_url_env, source)?,
        Err(openapi_err) => {
            tracing::info!("openapi 失败({openapi_err}),试 DRF OPTIONS...");
            discover_drf(base_url, token).await.with_context(|| format!("openapi 和 DRF 都失败;openapi: {openapi_err}"))?
        }
    };
    kb.domain = name.into();
    kb.source = source.into();
    kb.base_url_env = base_url_env.into();
    let count = kb.actions.len();
    let kb_path = write_kb(&kb)?;
    register_agent(name, description, &kb)?;
    Ok((count, kb_path))
}

/// LLM 从文档提取的 action(数组格式,转 KbAction 时 name 做 key)。
#[derive(Deserialize, Debug)]
struct LlmAction {
    name: String,
    method: String,
    path: String,
    #[serde(default)]
    description: String,
    class: String,
    #[serde(default)]
    params: Vec<KbParam>,
}

/// LLM 读文档发现:从 README/路由代码提取 API 端点 -> ServiceKb。覆盖无 openapi + 非标准 DRF 的服务(如 MusicTag)。
pub async fn discover_from_docs(name: &str, doc: &str, base_url_env: &str, source: &str, library: &ModelLibrary) -> Result<ServiceKb> {
    let core = &library.core;
    if core.provider.is_empty() || core.model.is_empty() {
        anyhow::bail!("未配 core LLM,无法从文档发现");
    }
    let provider = library.find_provider(&core.provider)
        .ok_or_else(|| anyhow::anyhow!("core provider '{}' 不在模型库", core.provider))?;
    let client = crate::llm::LlmClient::from_provider(provider, &core.model);
    let system = "你是 API 发现助手。从服务文档(README/路由代码/URL 列表)提取 REST API 端点。输出 JSON:{\"actions\":[{\"name\":\"snake_case 操作名\",\"method\":\"GET/POST/PUT/PATCH/DELETE\",\"path\":\"/api/...\",\"description\":\"中文说明\",\"class\":\"read/write/destructive\",\"params\":[{\"name\":\"参数名\",\"in\":\"query/path/body\",\"type\":\"string/integer\",\"required\":true}]}]}。class:GET=read,POST/PUT/PATCH=write,DELETE=destructive。只输出 JSON。";
    let user = format!("服务名:{name}\n\n文档:\n{doc}");
    let resp = client.chat(system, &user).await?;
    let json_str = crate::llm::extract_json(&resp)?;
    let parsed: Value = serde_json::from_str(&json_str).context("解析 LLM 输出 JSON 失败")?;
    let arr = parsed.get("actions").and_then(|a| a.as_array())
        .ok_or_else(|| anyhow::anyhow!("LLM 输出无 actions 数组"))?;
    let actions: Vec<LlmAction> = serde_json::from_value(Value::Array(arr.clone())).context("解析 actions 失败")?;
    let mut map = HashMap::new();
    for a in actions {
        map.insert(a.name, KbAction { method: a.method, path: a.path, description: a.description, class: a.class, params: a.params, response_path: String::new() });
    }
    if map.is_empty() {
        anyhow::bail!("LLM 未从文档提取出任何 action");
    }
    tracing::info!("discover_from_docs: {} 提取 {} 个 action", name, map.len());
    Ok(ServiceKb { domain: name.into(), source: source.into(), base_url_env: base_url_env.into(), auth: None, actions: map })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_openapi() -> Value {
        serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/api/platforms": {
                    "get": {"summary": "List platforms", "operationId": "listPlatforms", "parameters": []},
                    "post": {"summary": "Create platform", "operationId": "createPlatform"}
                },
                "/api/roms": {
                    "get": {"summary": "List games", "operationId": "listRoms", "parameters": [
                        {"name": "platform_id", "in": "query", "required": false, "schema": {"type": "integer"}}
                    ]}
                },
                "/api/roms/{id}": {
                    "get": {"summary": "Get game", "operationId": "getRom", "parameters": [
                        {"name": "id", "in": "path", "required": true, "schema": {"type": "integer"}}
                    ]},
                    "delete": {"summary": "Delete game", "operationId": "deleteRom"}
                },
                "/health": {"get": {"summary": "Health check"}},
                "/auth/login": {"post": {"summary": "Login"}}
            }
        })
    }

    #[test]
    fn openapi_to_kb_parses_actions() {
        let kb = openapi_to_kb(&sample_openapi(), "romm", "ROMM_URL", "rommapp/romm").unwrap();
        assert_eq!(kb.domain, "romm");
        assert_eq!(kb.base_url_env, "ROMM_URL");
        // /health 和 /auth/login 被过滤,剩 4 个 get/post/delete -> listPlatforms/createPlatform/listRoms/getRom/deleteRom = 5
        assert_eq!(kb.actions.len(), 5);
    }

    #[test]
    fn openapi_to_kb_class_inference() {
        let kb = openapi_to_kb(&sample_openapi(), "romm", "ROMM_URL", "x").unwrap();
        assert_eq!(kb.actions.get("listroms").unwrap().class, "read");
        assert_eq!(kb.actions.get("createplatform").unwrap().class, "write");
        assert_eq!(kb.actions.get("deleterom").unwrap().class, "destructive");
    }

    #[test]
    fn openapi_to_kb_path_param() {
        let kb = openapi_to_kb(&sample_openapi(), "romm", "ROMM_URL", "x").unwrap();
        let get_rom = kb.actions.get("getrom").unwrap();
        assert_eq!(get_rom.path, "/api/roms/{id}");
        assert_eq!(get_rom.method, "GET");
        assert_eq!(get_rom.params.len(), 1);
        assert_eq!(get_rom.params[0].location, "path");
        assert!(get_rom.params[0].required);
    }

    #[test]
    fn openapi_to_kb_skips_internal() {
        let kb = openapi_to_kb(&sample_openapi(), "romm", "ROMM_URL", "x").unwrap();
        // /health 和 /auth/login 不应出现
        assert!(kb.actions.values().all(|a| !a.path.starts_with("/health") && !a.path.starts_with("/auth")));
    }

    #[test]
    fn synthetic_name_fallback() {
        // 无 operationId 的端点用合成名
        let spec = serde_json::json!({"paths": {"/api/stats": {"get": {"summary": "Stats"}}}});
        let kb = openapi_to_kb(&spec, "x", "X_URL", "x").unwrap();
        assert!(kb.actions.contains_key("get_stats"));
    }

    #[test]
    fn empty_paths_rejected() {
        let spec = serde_json::json!({"paths": {}});
        assert!(openapi_to_kb(&spec, "x", "X", "x").is_err());
    }

    #[test]
    fn drf_root_parses_routes() {
        let root = serde_json::json!({"task": "http://x/api/task/", "record": "http://x/api/record/"});
        let routes = parse_drf_root(&root);
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|(k, _)| k == "task"));
    }

    #[test]
    fn drf_root_rejects_custom_format() {
        // MusicTag 自定义 DRF:{result,code,message,data} 非 DRF 根结构
        let root = serde_json::json!({"result": false, "code": "40101", "message": "未认证", "data": null});
        assert!(parse_drf_root(&root).is_empty());
    }

    #[test]
    fn drf_meta_to_actions_list_and_create() {
        let meta = serde_json::json!({
            "name": "Task List",
            "actions": {"POST": {"fields": {"title": {"type": "string", "required": true}}}}
        });
        let actions = drf_meta_to_actions("task", "/api/task/", &meta);
        assert_eq!(actions.len(), 2);
        let list = actions.iter().find(|(n, _)| n == "list_task").unwrap();
        assert_eq!(list.1.class, "read");
        let create = actions.iter().find(|(n, _)| n == "create_task").unwrap();
        assert_eq!(create.1.class, "write");
        assert_eq!(create.1.params.len(), 1);
        assert_eq!(create.1.params[0].name, "title");
        assert!(create.1.params[0].required);
    }
}
