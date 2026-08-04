//! homeassistant-agent - 智能家居控制(预设执行体)。
//!
//! 域:设备列表/状态查询/服务调用(开灯/调温等)。调 HA REST API。

use serde_json::{json, Value};
use std::time::Duration;
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct HomeAssistantAgent;

const HA_URL: &str = "http://localhost:8123";

impl HomeAssistantAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "list_entities" => self.list_entities().await,
            "get_state" => self.get_state(args).await,
            "call_service" => self.call_service(args).await,
            "list_services" => self.list_services().await,
            "list_automations" => self.list_automations().await,
            "fire_event" => self.fire_event(args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    fn token(&self) -> Option<String> {
        std::env::var("HA_TOKEN").ok().filter(|s| !s.is_empty())
    }

    async fn ha_get(&self, path: &str) -> Result<Value, String> {
        let token = self.token().ok_or("未配置 HA_TOKEN 环境变量")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let url = format!("{HA_URL}{path}");
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| format!("HA API 请求失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HA 返回 {}", resp.status()));
        }
        resp.json::<Value>()
            .await
            .map_err(|e| format!("解析失败: {e}"))
    }

    async fn ha_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let token = self.token().ok_or("未配置 HA_TOKEN 环境变量")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let url = format!("{HA_URL}{path}");
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("HA API 请求失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HA 返回 {}", resp.status()));
        }
        resp.json::<Value>()
            .await
            .map_err(|e| format!("解析失败: {e}"))
    }

    /// 列出所有设备状态
    async fn list_entities(&self) -> ActionResult {
        match self.ha_get("/api/states").await {
            Ok(data) => {
                let count = data.as_array().map(|a| a.len()).unwrap_or(0);
                ActionResult::ok(data, format!("HA 设备共 {count} 个"))
            }
            Err(e) => ActionResult::err(e),
        }
    }

    /// 查指定设备状态
    async fn get_state(&self, args: &Value) -> ActionResult {
        let entity_id = args.get("entity_id").and_then(|v| v.as_str()).unwrap_or("");
        if entity_id.is_empty() {
            return ActionResult::err("缺少 entity_id 参数");
        }
        match self.ha_get(&format!("/api/states/{entity_id}")).await {
            Ok(data) => ActionResult::ok(data, format!("设备 {entity_id} 状态")),
            Err(e) => ActionResult::err(e),
        }
    }

    /// 调用 HA 服务(如 light.turn_on / climate.set_temperature)
    async fn call_service(&self, args: &Value) -> ActionResult {
        let domain = args.get("domain").and_then(|v| v.as_str()).unwrap_or("");
        let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("");
        if domain.is_empty() || service.is_empty() {
            return ActionResult::err("缺少 domain/service 参数(如 domain=light, service=turn_on)");
        }
        let payload = args.get("payload").cloned().unwrap_or(json!({}));
        let path = format!("/api/services/{domain}/{service}");
        match self.ha_post(&path, &payload).await {
            Ok(data) => ActionResult::ok(data, format!("服务 {domain}.{service} 调用成功")),
            Err(e) => ActionResult::err(format!("服务调用失败: {e}")),
        }
    }

    /// 列出所有可用服务
    async fn list_services(&self) -> ActionResult {
        match self.ha_get("/api/services").await {
            Ok(data) => {
                let count = data.as_array().map(|a| a.len()).unwrap_or(0);
                ActionResult::ok(data, format!("HA 服务域共 {count} 个"))
            }
            Err(e) => ActionResult::err(e),
        }
    }

    /// 列出自动化规则
    async fn list_automations(&self) -> ActionResult {
        match self.ha_get("/api/states").await {
            Ok(data) => {
                let empty = vec![];
                let automations: Vec<&Value> = data.as_array().unwrap_or(&empty)
                    .iter()
                    .filter(|e| e.get("entity_id").and_then(|e| e.as_str()).map(|s| s.starts_with("automation.")).unwrap_or(false))
                    .collect();
                ActionResult::ok(json!(automations), format!("HA 自动化规则共 {} 个", automations.len()))
            }
            Err(e) => ActionResult::err(e),
        }
    }

    /// 触发事件(如自定义事件)
    async fn fire_event(&self, args: &Value) -> ActionResult {
        let event_type = args.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
        if event_type.is_empty() {
            return ActionResult::err("缺少 event_type 参数");
        }
        let payload = args.get("payload").cloned().unwrap_or(json!({}));
        match self.ha_post(&format!("/api/events/{event_type}"), &payload).await {
            Ok(data) => ActionResult::ok(data, format!("事件 {event_type} 已触发")),
            Err(e) => ActionResult::err(format!("触发事件失败: {e}")),
        }
    }
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let mut data = serde_json::json!({});
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("homeassistant", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(serde_json::json!({"analysis": analysis}), "智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}
