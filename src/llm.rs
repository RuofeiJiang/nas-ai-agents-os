//! LLM Core 调度 + 双向翻译。
//!
//! Core 的职责:NL -> 快速语言 Intent(选 agent + action)→ agent 执行 → 结果 -> NL。
//! LLM 只用在 Core 两端翻译;中间 agent 执行无 LLM。

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::agents::{dispatch, registry_text, ActionResult, Intent, Plan};
use crate::models::{ModelLibrary, Provider};

/// LLM HTTP 客户端(OpenAI 兼容 chat completions + function calling)
pub struct LlmClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn from_provider(provider: &Provider, model: &str) -> Self {
        let api_key = if provider.api_key_env.is_empty() {
            None
        } else {
            std::env::var(&provider.api_key_env)
                .ok()
                .filter(|s| !s.is_empty())
        };
        Self {
            base_url: provider.base_url.clone(),
            api_key,
            model: model.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("build reqwest client"),
        }
    }

    async fn post(&self, body: Value) -> Result<Value> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("POST {url} (model={})", self.model))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            // 脱敏:去掉可能的 key/token/bearer
            let safe_text = text.chars().take(500).collect::<String>()
                .replace("Bearer ", "Bearer ***")
                .replace(&std::env::var("ARK_API_KEY").unwrap_or_default(), "***")
                .replace(&std::env::var("HA_TOKEN").unwrap_or_default(), "***");
            anyhow::bail!("LLM {url} returned {status}: {safe_text}");
        }
        let json: Value = resp.json().await.context("decode chat response")?;
        json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .cloned()
            .context("no message in response")
    }

    /// 简单单轮(无工具),返回 content。
    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({"model": self.model, "messages": [{"role":"system","content":system},{"role":"user","content":user}], "temperature": 0.0});
        let msg = self.post(body).await?;
        Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string())
    }

    /// agent 循环:带 tools 调用,LLM 调工具 -> 架构执行 -> 喂回 -> 直到出最终文本。
    pub async fn chat_with_tools(&self, system: &str, user: &str, tools: &[Value], max_iters: usize) -> Result<String> {
        let mut messages = vec![
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": user}),
        ];
        for i in 0..max_iters {
            let body = json!({"model": self.model, "messages": messages, "tools": tools, "tool_choice": "auto", "temperature": 0.0});
            let msg = self.post(body).await?;
            let tool_calls = msg.get("tool_calls").and_then(|t| t.as_array());
            if let Some(tcs) = tool_calls {
                if tcs.is_empty() {
                    return Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string());
                }
                messages.push(msg.clone());
                for tc in tcs {
                    let fname = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                    let args_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}");
                    let args: Value = serde_json::from_str(args_str).unwrap_or(Value::Null);
                    let result = execute_tool(fname, &args).await;
                    let id = tc.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    tracing::info!("agent iter {i}: 工具 {fname}({args_str}) -> {}", result.chars().take(120).collect::<String>());
                    messages.push(json!({"role": "tool", "tool_call_id": id, "content": result}));
                }
                continue;
            }
            return Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string());
        }
        // 超过 max_iters,做最后一次无工具调用强制输出
        tracing::warn!("chat_with_tools 超过 {max_iters} 轮,强制无工具输出");
        let body = json!({"model": self.model, "messages": messages, "temperature": 0.0});
        let msg = self.post(body).await?;
        Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string())
    }
}

/// 工具定义:list_agents + get_system_context + check_state + get_task_pattern
fn tools() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{"name":"list_agents","description":"列出所有可用的 Execution Agent 及其 action","parameters":{"type":"object","properties":{}}}}),
        json!({"type":"function","function":{"name":"get_system_context","description":"查 NAS 系统配置(L2):目录/服务/策略/硬件","parameters":{"type":"object","properties":{}}}}),
        json!({"type":"function","function":{"name":"check_state","description":"查实时状态(L3):磁盘/内存/容器/SMART","parameters":{"type":"object","properties":{"area":{"type":"string","enum":["disk","smart","containers","all"]}},"required":["area"]}}}),
        json!({"type":"function","function":{"name":"get_task_pattern","description":"查任务调度模式","parameters":{"type":"object","properties":{"task":{"type":"string","description":"任务描述"}},"required":["task"]}}}),
    ]
}

/// 执行工具调用(async:check_state/execute_action 要调 OMV RPC/agent dispatch)。
async fn execute_tool(name: &str, args: &Value) -> String {
    match name {
        "list_agents" => registry_text(),
        "get_system_context" => system_context_text(),
        "check_state" => check_state(args).await,
        "get_task_pattern" => task_pattern_text(args),
        _ => format!("unknown tool: {name}"),
    }
}

/// 查任务调度模式(kb-tasks.json)
fn task_pattern_text(args: &Value) -> String {
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
    let text = std::fs::read_to_string("/etc/aaos/kb-tasks.json")
        .or_else(|_| std::fs::read_to_string("kb-tasks.json"))
        .unwrap_or_default();
    if text.is_empty() {
        return "[]".into();
    }
    // 返回全部模式让 LLM 自己匹配(模式不多)
    if task.is_empty() {
        return text;
    }
    // 简单按关键词过滤
    let patterns: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
    let task_lower = task.to_lowercase();
    let matched: Vec<&Value> = patterns.iter()
        .filter(|p| p.get("keywords").and_then(|k| k.as_array())
            .map(|kws| kws.iter().any(|k| k.as_str().map(|s| task_lower.contains(&s.to_lowercase())).unwrap_or(false)))
            .unwrap_or(false))
        .collect();
    if matched.is_empty() {
        text // 没匹配到就返回全部
    } else {
        serde_json::to_string(&matched).unwrap_or_default()
    }
}

/// L2 系统知识库(kb-context.json):这台 NAS 的专属配置
fn system_context_text() -> String {
    std::fs::read_to_string("/etc/aaos/kb-context.json")
        .or_else(|_| std::fs::read_to_string("kb-context.json"))
        .unwrap_or_default()
}

/// L3 实时状态:Core 不直接查,dispatch 给 system-agent/check_state 执行。
async fn check_state(args: &Value) -> String {
    let intent = Intent {
        agent: "system".into(),
        action: "check_state".into(),
        args: args.clone(),
    };
    let result = dispatch(&intent).await;
    if result.success {
        result.data.to_string()
    } else {
        format!("{{\"error\":\"{}\"}}", result.message)
    }
}

/// 从 LLM 回复里提取 JSON。
fn extract_json(text: &str) -> Result<String> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).map(|s| s.trim_start_matches('\n')).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    if let (Some(start), Some(end)) = (t.find('{'), t.rfind('}')) {
        if end >= start {
            return Ok(t[start..=end].to_string());
        }
    }
    anyhow::bail!("LLM 输出里找不到 JSON: {t}")
}

/// 解析 LLM 输出为 Intent。
pub fn parse_intent(text: &str) -> Result<Intent> {
    let json_str = extract_json(text)?;
    let v: Value = serde_json::from_str(&json_str).context("parse Intent JSON")?;
    parse_intent_value(&v)
}

/// 从 JSON Value 解析 Intent
fn parse_intent_value(v: &Value) -> Result<Intent> {
    let agent = v.get("agent").and_then(|x| x.as_str()).context("missing 'agent'")?.to_string();
    let action = v.get("action").and_then(|x| x.as_str()).context("missing 'action'")?.to_string();
    if agent.is_empty() || action.is_empty() {
        anyhow::bail!("LLM 未能调度(agent/action 为空)");
    }
    let args = v.get("args").cloned().unwrap_or(Value::Null);
    Ok(Intent { agent, action, args })
}

/// 解析 LLM 输出为 Plan(多步)。若不含 "steps" 则返回 None(可能是单 Intent)。
pub fn parse_plan(text: &str) -> Result<Option<Plan>> {
    let json_str = extract_json(text)?;
    let v: Value = serde_json::from_str(&json_str).context("parse JSON")?;
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        let mut intents = Vec::new();
        for step in steps {
            intents.push(parse_intent_value(step)?);
        }
        Ok(Some(Plan { steps: intents }))
    } else {
        Ok(None)
    }
}

/// Core 正向:NL -> LLM 原始输出(可能是 Intent JSON,也可能是直接回复)。
/// 没配 core.model 返回 None(回退规则)。由 core.rs parse_intent 判断。
pub async fn schedule_to_intent(input: &str, library: &ModelLibrary) -> Result<Option<String>> {
    let core = &library.core;
    if core.provider.is_empty() || core.model.is_empty() {
        return Ok(None);
    }
    let provider = library
        .find_provider(&core.provider)
        .ok_or_else(|| anyhow::anyhow!("core provider '{}' 不在模型库", core.provider))?;
    let client = LlmClient::from_provider(provider, &core.model);
    let system = "你是 NAS 调度助手(Core Agent)。用工具查知识库完成任务调度:\n- list_agents:查 agent+action\n- get_system_context:查 NAS 配置(L2)\n- check_state:查实时状态(L3)\n- get_task_pattern:查任务调度模式(复杂任务该拆成哪些步骤)\n\n查完后输出调度决策:\n- 单步:{\"agent\":\"...\",\"action\":\"...\",\"args\":{}}\n- 多步:{\"steps\":[...]}\n- 破坏性操作:输出 Intent 让用户确认\n- 预检不过:直接回复原因";
    let tools = tools();
    let resp = client.chat_with_tools(system, input, &tools, 5).await?;
    if resp.trim().is_empty() {
        anyhow::bail!("LLM 返回空");
    }
    Ok(Some(resp))
}

/// Core 反向:ActionResult -> NL(把结构化结果翻成人话给用户)。
pub async fn result_to_nl(input: &str, result: &ActionResult, library: &ModelLibrary) -> Result<String> {
    let core = &library.core;
    if core.provider.is_empty() || core.model.is_empty() {
        // 无 LLM,返回结构化摘要
        return Ok(format!("{} | {}", result.message, result.data));
    }
    let provider = library
        .find_provider(&core.provider)
        .ok_or_else(|| anyhow::anyhow!("core provider '{}' 不在模型库", core.provider))?;
    let client = LlmClient::from_provider(provider, &core.model);
    let system = "你是 NAS 助手。用户用自然语言提问,系统执行后返回了结构化结果(JSON)。请基于结果用自然语言简洁回答用户,不要照搬 JSON,提炼关键信息。若结果为空/失败,如实说明。";
    let user = format!("用户问:{input}\n执行结果(success={}, message={}):\n{}", result.success, result.message, result.data);
    let resp = client.chat(system, &user).await?;
    Ok(resp)
}

/// 测试 LLM 连通性。
pub async fn test_connection(library: &ModelLibrary) -> Result<String> {
    let core = &library.core;
    if core.provider.is_empty() || core.model.is_empty() {
        anyhow::bail!("[core] 未配 LLM");
    }
    let provider = library
        .find_provider(&core.provider)
        .ok_or_else(|| anyhow::anyhow!("core provider '{}' 不在模型库", core.provider))?;
    let client = LlmClient::from_provider(provider, &core.model);
    client.chat("你是连通性测试助手。", "reply: OK").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_intent_plain() {
        let r = parse_intent(r#"{"agent":"system","action":"check_smart","args":{}}"#).unwrap();
        assert_eq!(r.agent, "system");
        assert_eq!(r.action, "check_smart");
    }

    #[test]
    fn parse_intent_in_codeblock() {
        let r = parse_intent("```json\n{\"agent\":\"system\",\"action\":\"system_info\",\"args\":{}}\n```").unwrap();
        assert_eq!(r.action, "system_info");
    }

    #[test]
    fn parse_intent_rejects_empty() {
        assert!(parse_intent(r#"{"agent":"","action":""}"#).is_err());
    }

    #[test]
    fn extract_json_markdown() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```").unwrap(), "{\"a\":1}");
    }

    #[tokio::test]
    async fn schedule_returns_none_without_core_model() {
        let toml = r#"
current = "p"
[[providers]]
id = "p"
name = "T"
provider_type = "openai"
base_url = "http://x"
[core]
"#;
        let lib = crate::models::ModelLibrary::from_str(toml).unwrap();
        assert!(schedule_to_intent("磁盘健康", &lib).await.unwrap().is_none());
    }
}
