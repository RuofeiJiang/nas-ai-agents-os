//! Execution Agents - 预设执行体。
//!
//! 吃快速语言 [`Intent`](Core 翻译 NL 而来),用域 KB 执行,吐 [`ActionResult`]。
//! Core 调度选 agent,agent 预设执行,不碰 NL。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod backup;
pub mod cloud;
pub mod docker;
pub mod download;
pub mod filesystem;
pub mod ha;
pub mod llm_helper;
pub mod media;
pub mod system;
pub mod vision;

/// 快速语言:Core -> Agent 的结构化意图
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// agent 名,如 "system" / "docker"
    pub agent: String,
    /// 动作,如 "check_smart"
    pub action: String,
    /// 参数(一般 {})
    #[serde(default)]
    pub args: Value,
}

/// 多步计划(Core 分解复杂任务)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub steps: Vec<Intent>,
}

/// OMV 新建对象的哨兵 UUID(OMV_CONFIGOBJECT_NEW_UUID)
pub const OMV_NEW_UUID: &str = "fa4b1c66-ef79-11e5-87a0-0002b3a176b4";

/// 快速语言:Agent -> Core 的结构化结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub data: Value,
    pub message: String,
}

impl ActionResult {
    pub fn ok(data: Value, message: impl Into<String>) -> Self {
        Self { success: true, data, message: message.into() }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self { success: false, data: Value::Null, message: message.into() }
    }
}

/// 调度:把 Intent 路由到对应 Execution Agent 执行。
pub async fn dispatch(intent: &Intent) -> ActionResult {
    tracing::info!(
        "dispatch -> agent={} action={} args={}",
        intent.agent, intent.action, intent.args
    );
    match intent.agent.as_str() {
        "system" => system::SystemAgent.execute(&intent.action, &intent.args).await,
        "docker" => docker::DockerAgent.execute(&intent.action, &intent.args).await,
        "filesystem" => filesystem::FilesystemAgent.execute(&intent.action, &intent.args).await,
        "backup" => backup::BackupAgent.execute(&intent.action, &intent.args).await,
        "vision" => vision::VisionAgent.execute(&intent.action, &intent.args).await,
        "media" => media::MediaAgent.execute(&intent.action, &intent.args).await,
        "download" => download::DownloadAgent.execute(&intent.action, &intent.args).await,
        "homeassistant" => ha::HomeAssistantAgent.execute(&intent.action, &intent.args).await,
        "cloud" => cloud::CloudAgent.execute(&intent.action, &intent.args).await,
        other => ActionResult::err(format!("未知 agent: {other}")),
    }
}

/// 多步计划调度:顺序执行每步,失败则停,收集所有结果。
pub async fn dispatch_plan(plan: &Plan) -> Vec<ActionResult> {
    let mut results = Vec::new();
    for (i, intent) in plan.steps.iter().enumerate() {
        tracing::info!("Plan step {}/{}: agent={} action={}", i + 1, plan.steps.len(), intent.agent, intent.action);
        let result = dispatch(intent).await;
        let success = result.success;
        results.push(result);
        if !success {
            tracing::warn!("Plan step {} 失败,停止后续步骤", i + 1);
            break;
        }
    }
    results
}

/// agent 注册表(Core 的 LLM 路由用):返回 agents.json 文本。
pub fn registry_text() -> String {
    std::fs::read_to_string("/etc/aaos/agents.json")
        .or_else(|_| std::fs::read_to_string("agents.json"))
        .unwrap_or_else(|_| "[{\"name\":\"system\",\"actions\":[]}]".into())
}
