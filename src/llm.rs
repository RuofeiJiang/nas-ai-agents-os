//! LLM Core 调度 + 双向翻译。
//!
//! Core 的职责:NL -> 快速语言 Intent(选 agent + action)→ agent 执行 → 结果 -> NL。
//! LLM 只用在 Core 两端翻译;中间 agent 执行无 LLM。

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::agents::{dispatch, registry_text, ActionResult, Intent, Plan};
use crate::models::{ModelLibrary, Provider};

/// LLM HTTP 客户端。协议差异在此处收敛，Core 只看到文本和工具循环。
pub struct LlmClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    provider_type: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    input: Value,
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
            provider_type: provider.provider_type.to_lowercase(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("build reqwest client"),
        }
    }

    fn is_anthropic(&self) -> bool {
        self.provider_type == "anthropic"
    }

    fn safe_error(&self, text: &str) -> String {
        let mut safe = text.chars().take(500).collect::<String>()
            .replace("Bearer ", "Bearer ***");
        if let Some(key) = &self.api_key {
            if !key.is_empty() { safe = safe.replace(key, "***"); }
        }
        safe
    }

    async fn post_openai(&self, body: Value) -> Result<Value> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key { req = req.bearer_auth(key); }
        let resp = req.send().await.with_context(|| format!("POST {url} (model={})", self.model))?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("LLM provider={} model={} POST {url} returned {status}: {}", self.provider_type, self.model, self.safe_error(&resp.text().await.unwrap_or_default()));
        }
        let body: Value = resp.json().await.context("decode chat response")?;
        body.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("message")).cloned().context("no message in OpenAI response")
    }

    async fn post_anthropic(&self, body: Value) -> Result<Value> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let mut req = self.http.post(&url).json(&body).header("anthropic-version", "2023-06-01");
        if let Some(key) = &self.api_key { req = req.header("x-api-key", key); }
        let resp = req.send().await.with_context(|| format!("POST {url} (model={})", self.model))?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("LLM provider={} model={} POST {url} returned {status}: {}", self.provider_type, self.model, self.safe_error(&resp.text().await.unwrap_or_default()));
        }
        resp.json().await.context("decode Anthropic response")
    }

    /// 简单单轮(无工具),返回 content。
    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        if self.is_anthropic() {
            let body = json!({"model": self.model, "max_tokens": 2048, "system": system, "messages": [{"role":"user","content": user}], "thinking": {"type":"adaptive"}});
            return anthropic_text(&self.post_anthropic(body).await?);
        }
        let msg = self.post_openai(json!({"model": self.model, "messages": [{"role":"system","content":system},{"role":"user","content":user}], "temperature": 0.0})).await?;
        Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string())
    }

    /// agent 循环:协议无关的工具调用，工具执行仍完全由 AAOS 架构控制。
    pub async fn chat_with_tools(&self, system: &str, user: &str, tools: &[Value], max_iters: usize) -> Result<String> {
        if self.is_anthropic() { return self.chat_with_tools_anthropic(system, user, tools, max_iters).await; }
        let mut messages = vec![json!({"role":"system","content":system}), json!({"role":"user","content":user})];
        for i in 0..max_iters {
            let msg = self.post_openai(json!({"model": self.model, "messages": messages, "tools": tools, "tool_choice": "auto", "temperature": 0.0})).await?;
            let calls = openai_tool_calls(&msg)?;
            if calls.is_empty() { return Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string()); }
            messages.push(msg);
            for call in calls {
                let result = execute_tool(&call.name, &call.input).await;
                tracing::info!("agent iter {i}: 工具 {}({}) -> {}", call.name, call.input, result.chars().take(120).collect::<String>());
                messages.push(json!({"role":"tool","tool_call_id":call.id,"content":result}));
            }
        }
        tracing::warn!("chat_with_tools 超过 {max_iters} 轮,强制无工具输出");
        let msg = self.post_openai(json!({"model": self.model, "messages": messages, "temperature": 0.0})).await?;
        Ok(msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string())
    }

    async fn chat_with_tools_anthropic(&self, system: &str, user: &str, tools: &[Value], max_iters: usize) -> Result<String> {
        let mut messages = vec![json!({"role":"user","content": user})];
        let anthropic_tools: Vec<Value> = tools.iter().map(to_anthropic_tool).collect();
        for i in 0..max_iters {
            let body = json!({"model": self.model, "max_tokens": 4096, "system": system, "messages": messages, "tools": anthropic_tools, "tool_choice": {"type":"auto"}, "thinking": {"type":"adaptive"}});
            let response = self.post_anthropic(body).await?;
            let calls = anthropic_tool_calls(&response)?;
            if calls.is_empty() { return anthropic_text(&response); }
            // Anthropic requires the complete assistant content blocks to be replayed.
            let content = response.get("content").cloned().context("Anthropic response missing content")?;
            messages.push(json!({"role":"assistant","content":content}));
            let mut results = Vec::new();
            for call in calls {
                let result = execute_tool(&call.name, &call.input).await;
                tracing::info!("agent iter {i}: 工具 {}({}) -> {}", call.name, call.input, result.chars().take(120).collect::<String>());
                results.push(json!({"type":"tool_result","tool_use_id":call.id,"content":result}));
            }
            messages.push(json!({"role":"user","content":results}));
        }
        anyhow::bail!("Anthropic tool loop exceeded {max_iters} iterations (provider={} model={})", self.provider_type, self.model)
    }
}

fn to_anthropic_tool(tool: &Value) -> Value {
    let f = tool.get("function").unwrap_or(tool);
    json!({"name": f.get("name").and_then(Value::as_str).unwrap_or(""), "description": f.get("description").and_then(Value::as_str).unwrap_or(""), "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{},"additionalProperties":false}))})
}

fn openai_tool_calls(msg: &Value) -> Result<Vec<ToolCall>> {
    let mut out = Vec::new();
    for tc in msg.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default() {
        let f = tc.get("function").context("OpenAI tool call missing function")?;
        let name = f.get("name").and_then(Value::as_str).context("OpenAI tool call missing name")?;
        let raw = f.get("arguments").and_then(Value::as_str).unwrap_or("{}");
        let input = serde_json::from_str(raw).context("malformed OpenAI tool arguments")?;
        out.push(ToolCall { id: tc.get("id").and_then(Value::as_str).unwrap_or("").to_string(), name: name.to_string(), input });
    }
    Ok(out)
}

fn anthropic_tool_calls(response: &Value) -> Result<Vec<ToolCall>> {
    let mut out = Vec::new();
    for block in response.get("content").and_then(Value::as_array).cloned().unwrap_or_default() {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") { continue; }
        out.push(ToolCall { id: block.get("id").and_then(Value::as_str).context("Anthropic tool_use missing id")?.to_string(), name: block.get("name").and_then(Value::as_str).context("Anthropic tool_use missing name")?.to_string(), input: block.get("input").cloned().unwrap_or_else(|| json!({})) });
    }
    Ok(out)
}

fn anthropic_text(response: &Value) -> Result<String> {
    let text: String = response.get("content").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|b| if b.get("type").and_then(Value::as_str) == Some("text") { b.get("text").and_then(Value::as_str).map(str::to_string) } else { None }).collect::<Vec<_>>().join("");
    if text.is_empty() && response.get("stop_reason").and_then(Value::as_str) == Some("refusal") { anyhow::bail!("Anthropic model refusal: {}", response); }
    Ok(text)
}

/// 工具定义:list_agents + get_system_context + check_state + get_task_pattern
fn tools() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{"name":"list_agents","description":"列出所有可用的 Execution Agent 及其 action","parameters":{"type":"object","properties":{}}}}),
        json!({"type":"function","function":{"name":"get_system_context","description":"查 L1 系统上下文:明确 NAS 任务目标、目录、服务、策略、硬件","parameters":{"type":"object","properties":{}}}}),
        json!({"type":"function","function":{"name":"search_knowledge","description":"查 L2 知识架构:按请求查询确定性 KB、LLM 内在知识或必要的网络知识；返回来源与可信度","parameters":{"type":"object","properties":{"query":{"type":"string"},"source":{"type":"string","enum":["kb","llm","web","auto"]}},"required":["query","source"]}}}),
        json!({"type":"function","function":{"name":"check_state","description":"查 L3 系统实时状态:磁盘/内存/容器/SMART,确认是否具备执行条件","parameters":{"type":"object","properties":{"area":{"type":"string","enum":["disk","smart","containers","all"]}},"required":["area"]}}}),
        json!({"type":"function","function":{"name":"get_task_pattern","description":"查任务调度模式","parameters":{"type":"object","properties":{"task":{"type":"string","description":"任务描述"}},"required":["task"]}}}),
    ]
}

/// 执行工具调用(async:check_state/execute_action 要调 OMV RPC/agent dispatch)。
async fn execute_tool(name: &str, args: &Value) -> String {
    match name {
        "list_agents" => registry_text(),
        "get_system_context" => system_context_text(),
        "search_knowledge" => search_knowledge(args).await,
        "check_state" => check_state(args).await,
        "get_task_pattern" => task_pattern_text(args),
        _ => format!("unknown tool: {name}"),
    }
}

/// L2 知识架构的统一入口。当前先查询确定性 KB；LLM/web 由后续 provider/tool 接入。
async fn search_knowledge(args: &Value) -> String {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let source = args.get("source").and_then(Value::as_str).unwrap_or("auto");
    let files = ["/etc/aaos/kb-nas.json", "/etc/aaos/kb-context.json", "/etc/aaos/kb-tasks.json", "/etc/aaos/kb-download.json"];
    let mut hits = Vec::new();
    for path in files {
        if let Ok(text) = std::fs::read_to_string(path).or_else(|_| std::fs::read_to_string(path.trim_start_matches("/etc/aaos/"))) {
            if query.is_empty() || text.to_lowercase().contains(&query.to_lowercase()) {
                hits.push(json!({"source":"kb","file":path,"content":text}));
            }
        }
    }
    if hits.is_empty() && source == "kb" { return json!({"source":"kb","found":false,"query":query}).to_string(); }
    json!({"source":"kb","query":query,"results":hits,"note":"LLM/web knowledge requires an explicit provider adapter; never execute unverified external text directly"}).to_string()
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

/// 查 L1 系统上下文(kb-context.json):明确本机任务目标与对象。
fn system_context_text() -> String {
    std::fs::read_to_string("/etc/aaos/kb-context.json")
        .or_else(|_| std::fs::read_to_string("kb-context.json"))
        .unwrap_or_default()
}

/// 查 L3 系统实时状态:Core 不直接查,dispatch 给 system-agent/check_state 执行。
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
pub fn extract_json(text: &str) -> Result<String> {
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
/// 主模型失败时按配置尝试 fallback；只有 fallback 也失败才回退规则。
pub async fn schedule_to_intent(input: &str, library: &ModelLibrary) -> Result<Option<String>> {
    let core = &library.core;
    if core.provider.is_empty() || core.model.is_empty() { return Ok(None); }
    let system = "你是 NAS 调度助手(Core Agent)。正式执行前必须按三阶段预检:\n- L1 系统上下文:明确用户目标、对象和本机相关配置\n- L2 查询知识架构:优先查确定性 KB；不足时标记需要 LLM 推理或网络搜索，并保留来源/可信度\n- L3 系统实时状态:确认当前是否具备执行条件\n工具:\n- list_agents:查 agent+action\n- get_system_context:查 L1 系统上下文\n- search_knowledge:查 L2 知识架构\n- check_state:查 L3 实时状态\n- get_task_pattern:查任务调度模式(复杂任务该拆成哪些步骤)\n\n查完后输出调度决策:\n- 单步:{\"agent\":\"...\",\"action\":\"...\",\"args\":{}}\n- 多步:{\"steps\":[...]}\n- 破坏性操作:输出 Intent 让用户确认\n- 预检不过:直接回复原因";
    let tools = tools();
    let provider = library.find_provider(&core.provider).ok_or_else(|| anyhow::anyhow!("core provider '{}' 不在模型库", core.provider))?;
    let primary = LlmClient::from_provider(provider, &core.model);
    match primary.chat_with_tools(system, input, &tools, 5).await {
        Ok(resp) => {
            if resp.trim().is_empty() { anyhow::bail!("LLM 返回空"); }
            return Ok(Some(resp));
        },
        Err(err) => {
            tracing::warn!("LLM 主模型失败 provider={} model={} error={:#}", core.provider, core.model, err);
            if !core.fallback_provider.is_empty() && !core.fallback_model.is_empty() {
                if let Some(fp) = library.find_provider(&core.fallback_provider) {
                    let fallback = LlmClient::from_provider(fp, &core.fallback_model);
                    match fallback.chat_with_tools(system, input, &tools, 5).await {
                        Ok(resp) => { tracing::info!("LLM fallback 成功 provider={} model={}", core.fallback_provider, core.fallback_model); return Ok(Some(resp)); }
                        Err(ferr) => tracing::error!("LLM fallback 失败 provider={} model={} error={:#}", core.fallback_provider, core.fallback_model, ferr),
                    }
                }
            }
            Err(err)
        }
    }
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
