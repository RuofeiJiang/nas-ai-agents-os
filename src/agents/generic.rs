//! generic-http-agent - 通用 HTTP 执行器(数据驱动)。
//!
//! 不硬编码任何服务。行为由 `/etc/aaos/kb-<service>.json` 描述:
//! 每个 action 映射到 HTTP method+path+params+auth+class。
//! docker-agent 部署服务后自动发现 API 生成 KB,动态注册到此 agent。
//!
//! 调度:Core 输出 Intent{agent:"romm", action:"list_games"} ->
//!   dispatch 未知 agent fallback 到此 -> 读 kb-romm.json -> 调 HTTP -> ActionResult。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

use crate::agents::{llm_helper, ActionResult};
use crate::models::ModelLibrary;

pub struct GenericHttpAgent;

/// 服务知识库(kb-<service>.json 的 schema)
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ServiceKb {
    pub domain: String,
    #[serde(default)]
    pub source: String,
    /// 存 base_url 的环境变量名(如 ROMM_URL)
    pub base_url_env: String,
    #[serde(default)]
    pub auth: Option<Auth>,
    pub actions: HashMap<String, KbAction>,
}

/// 鉴权配置
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Auth {
    #[serde(rename = "type")]
    pub auth_type: String, // bearer / api_key / basic / none
    /// 存 token 的环境变量名(如 ROMM_TOKEN)
    pub token_env: String,
    #[serde(default)]
    pub header: String, // header 名,默认 Authorization
    #[serde(default)]
    pub prefix: String, // 如 "Bearer"
}

/// 单个 action -> HTTP 调用映射
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct KbAction {
    pub method: String, // GET / POST / PUT / PATCH / DELETE
    pub path: String,   // /api/roms 或 /api/roms/{id}
    pub description: String,
    pub class: String, // read / write / destructive
    #[serde(default)]
    pub params: Vec<KbParam>,
    #[serde(default)]
    pub response_path: String, // 提取响应里的字段,如 "data";空则返回整体
}

/// action 参数
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct KbParam {
    pub name: String,
    #[serde(rename = "in", default)]
    pub location: String, // query / path / body
    #[serde(rename = "type", default)]
    pub param_type: String, // string / integer / boolean / object
    #[serde(default)]
    pub required: bool,
}

impl GenericHttpAgent {
    /// 执行:agent_name 决定读哪个 KB,action 决定调哪个端点。
    pub async fn execute(&self, agent_name: &str, action: &str, args: &Value) -> ActionResult {
        let kb = match load_kb(agent_name) {
            Ok(k) => k,
            Err(e) => return ActionResult::err(format!("加载 {agent_name} KB 失败: {e}")),
        };
        match kb.actions.get(action) {
            Some(act) => self.call_http(&kb, act, args).await,
            None => self.smart_execute(agent_name, &kb, action, args).await,
        }
    }

    /// 按 KB 描述组装 HTTP 请求并调用。
    async fn call_http(&self, kb: &ServiceKb, act: &KbAction, args: &Value) -> ActionResult {
        let base_url = std::env::var(&kb.base_url_env).unwrap_or_default();
        if base_url.is_empty() {
            return ActionResult::err(format!("未配置 {} 环境变量", kb.base_url_env));
        }

        // path 参数替换
        let path = match replace_path_params(&act.path, args, &act.params) {
            Ok(p) => p,
            Err(e) => return ActionResult::err(e),
        };
        // query 参数
        let query = build_query(args, &act.params);
        // body 参数(POST/PUT/PATCH)
        let body = if matches!(act.method.to_uppercase().as_str(), "POST" | "PUT" | "PATCH") {
            act.params.iter().find(|p| p.location == "body").and_then(|bp| args.get(&bp.name)).cloned()
        } else {
            None
        };

        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(30)).build() {
            Ok(c) => c,
            Err(e) => return ActionResult::err(format!("HTTP 客户端构建失败: {e}")),
        };
        let mut req = match act.method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            m => return ActionResult::err(format!("不支持的 method: {m}")),
        };
        if !query.is_empty() {
            req = req.query(&query);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        req = apply_auth(req, &kb.auth);

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return ActionResult::err(format!("请求 {url} 失败: {e}")),
        };
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let snippet = body.to_string().chars().take(300).collect::<String>();
            return ActionResult::err(format!("{} {} 返回 {}: {}", act.method, path, status, snippet));
        }
        let data = extract_response(&body, &act.response_path);
        ActionResult::ok(data, format!("{}: {}", act.description, status))
    }

    /// KB 未覆盖的 action -> 域 LLM 分析(和现有 9 agent 一致)
    async fn smart_execute(&self, agent_name: &str, kb: &ServiceKb, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let available: Vec<&String> = kb.actions.keys().collect();
        let data = json!({"agent": agent_name, "available_actions": available});
        let library = match ModelLibrary::load() {
            Ok(l) => l,
            Err(e) => return ActionResult::err(format!("模型库: {e}")),
        };
        match llm_helper::smart_analyze(agent_name, user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(json!({"analysis": analysis}), format!("{agent_name} 智能分析完成")),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}

/// 加载服务 KB:/etc/aaos/kb-<name>.json 回退本地 kb-<name>.json
pub fn load_kb(name: &str) -> Result<ServiceKb> {
    let text = std::fs::read_to_string(format!("/etc/aaos/kb-{name}.json"))
        .or_else(|_| std::fs::read_to_string(format!("kb-{name}.json")))
        .with_context(|| format!("找不到 kb-{name}.json"))?;
    serde_json::from_str(&text).with_context(|| format!("解析 kb-{name}.json"))
}

/// path 参数替换:/api/roms/{id} + {id:5} -> /api/roms/5。缺必填 path 参数报错。
pub fn replace_path_params(path: &str, args: &Value, params: &[KbParam]) -> Result<String, String> {
    let mut out = path.to_string();
    for p in params.iter().filter(|p| p.location == "path") {
        let val = arg_to_string(args, &p.name);
        if val.is_empty() {
            if p.required {
                return Err(format!("缺少 path 参数: {}", p.name));
            }
            continue;
        }
        out = out.replace(&format!("{{{}}}", p.name), &val);
    }
    Ok(out)
}

/// 组装 query 参数:(name, value) 列表,跳过空值。
pub fn build_query(args: &Value, params: &[KbParam]) -> Vec<(String, String)> {
    let mut q = Vec::new();
    for p in params.iter().filter(|p| p.location == "query") {
        let val = arg_to_string(args, &p.name);
        if !val.is_empty() {
            q.push((p.name.clone(), val));
        }
    }
    q
}

/// 按 auth 配置注入鉴权 header。token 缺失则不注入(让服务端拒绝,报错可见)。
pub fn apply_auth(mut req: reqwest::RequestBuilder, auth: &Option<Auth>) -> reqwest::RequestBuilder {
    if let Some(a) = auth {
        if a.auth_type == "none" || a.auth_type.is_empty() {
            return req;
        }
        let token = std::env::var(&a.token_env).unwrap_or_default();
        if token.is_empty() {
            return req;
        }
        let header = if a.header.is_empty() { "Authorization" } else { a.header.as_str() };
        let val = if a.prefix.is_empty() { token } else { format!("{} {}", a.prefix, token) };
        req = req.header(header, val);
    }
    req
}

/// 从响应体提取 response_path 字段;空则返回整体。
pub fn extract_response(body: &Value, path: &str) -> Value {
    if path.is_empty() {
        return body.clone();
    }
    body.get(path).cloned().unwrap_or_else(|| body.clone())
}

/// 把 args 里的某个参数转成字符串(支持 string/integer/boolean)。
fn arg_to_string(args: &Value, name: &str) -> String {
    match args.get(name) {
        Some(Value::String(s)) => s.clone(),
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb_romm_sample() -> ServiceKb {
        let json = r#"{
            "domain": "romm",
            "source": "rommapp/romm",
            "base_url_env": "ROMM_URL",
            "auth": {"type":"bearer","token_env":"ROMM_TOKEN","header":"Authorization","prefix":"Bearer"},
            "actions": {
                "list_games": {"method":"GET","path":"/api/roms","description":"列出游戏","class":"read","params":[{"name":"platform_id","in":"query","type":"integer","required":false}],"response_path":"data"},
                "scan_library": {"method":"POST","path":"/api/platforms/{platform_id}/scan","description":"扫描库","class":"write","params":[{"name":"platform_id","in":"path","type":"integer","required":true}]}
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn kb_parse() {
        let kb = kb_romm_sample();
        assert_eq!(kb.domain, "romm");
        assert_eq!(kb.actions.len(), 2);
        assert_eq!(kb.auth.as_ref().unwrap().auth_type, "bearer");
    }

    #[test]
    fn path_param_replace() {
        let kb = kb_romm_sample();
        let act = kb.actions.get("scan_library").unwrap();
        let args = json!({"platform_id": 7});
        let p = replace_path_params(&act.path, &args, &act.params).unwrap();
        assert_eq!(p, "/api/platforms/7/scan");
    }

    #[test]
    fn path_param_missing_required() {
        let kb = kb_romm_sample();
        let act = kb.actions.get("scan_library").unwrap();
        let err = replace_path_params(&act.path, &json!({}), &act.params);
        assert!(err.is_err());
    }

    #[test]
    fn query_build_skips_empty() {
        let kb = kb_romm_sample();
        let act = kb.actions.get("list_games").unwrap();
        let q = build_query(&json!({"platform_id": 3}), &act.params);
        assert_eq!(q, vec![("platform_id".to_string(), "3".to_string())]);
        // 空 query 参数跳过
        let q2 = build_query(&json!({}), &act.params);
        assert!(q2.is_empty());
    }

    #[test]
    fn response_extract() {
        let body = json!({"data": [1, 2, 3], "count": 3});
        assert_eq!(extract_response(&body, "data"), json!([1, 2, 3]));
        // 空 path 返回整体
        assert_eq!(extract_response(&body, ""), body);
        // 不存在的 path 返回整体(fallback)
        assert_eq!(extract_response(&body, "missing"), body);
    }

    #[test]
    fn auth_none_no_header() {
        // auth=none 不应注入(token 即使存在也不注入)
        // 用一个假 client 验证不 panic
        let auth = Some(Auth { auth_type: "none".into(), token_env: "X".into(), header: "".into(), prefix: "".into() });
        let client = reqwest::Client::new();
        let req = client.get("http://x");
        let _ = apply_auth(req, &auth); // 不 panic 即可
    }
}
