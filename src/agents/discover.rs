//! discover.rs - 服务 API 发现 + OpenAPI->KB 转换。
//!
//! docker-agent 部署容器后,调 discover_service 探测 OpenAPI,
//! openapi_to_kb 转成 ServiceKb,写 kb-<service>.json + 注册 agent(动态纳管)。
//!
//! 三档发现:① 模板库(已知服务,后续)② OpenAPI 自动(本模块)③ LLM 读文档(兜底,后续)。

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

use crate::agents::generic::{KbAction, KbParam, ServiceKb};

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
pub async fn discover_service(base_url: &str) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut last_err = String::new();
    for path in OPENAPI_PATHS {
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = resp.json().await.with_context(|| format!("解析 {url}"))?;
                if body.get("paths").is_some() {
                    tracing::info!("发现 OpenAPI: {url}");
                    return Ok(body);
                }
                last_err = format!("{path}: 无 paths 字段");
            }
            Ok(resp) => last_err = format!("{path} -> {}", resp.status()),
            Err(e) => last_err = format!("{path} -> {e}"),
        }
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

/// 完整流程:发现 OpenAPI -> 转 KB -> 写文件 -> 注册 agent。返回 (action 数, kb 路径)。
pub async fn discover_and_register(name: &str, base_url: &str, base_url_env: &str, source: &str, description: &str) -> Result<(usize, String)> {
    let spec = discover_service(base_url).await?;
    let kb = openapi_to_kb(&spec, name, base_url_env, source)?;
    let count = kb.actions.len();
    let kb_path = write_kb(&kb)?;
    register_agent(name, description, &kb)?;
    Ok((count, kb_path))
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
}
