//! Agent LLM helper -- Execution Agent 的域 LLM 智能分析。
//!
//! 当 agent 的预设 KB 没有匹配的 action 时,用域 LLM 分析数据。
//! 域模型从 models.toml 的 [[execution_agent]] 配置取。

use anyhow::Result;
use serde_json::Value;

use crate::llm::LlmClient;
use crate::models::ModelLibrary;

/// 用域 LLM 分析数据,返回智能结果文本。
///
/// - `domain`: agent 域名,如 "system" / "docker" / "filesystem"
/// - `user_request`: 用户原始请求(Core 传过来的)
/// - `data`: agent 收集的域数据(JSON)
/// - `library`: 模型库(取 execution_agent 配的域模型)
pub async fn smart_analyze(
    domain: &str,
    user_request: &str,
    data: &Value,
    library: &ModelLibrary,
) -> Result<String> {
    // 从 models.toml 找该域的 execution_agent 配置
    let (provider, model) = match find_domain_model(domain, library) {
        Some((p, m)) => (p, m),
        None => {
            // 回退到 core 模型
            tracing::warn!("域 {domain} 未配 execution_agent 模型,回退 core 模型");
            match find_core_model(library) {
                Some((p, m)) => (p, m),
                None => return Ok(format!("无法分析: 未配置 LLM 模型。原始数据:\n{data}")),
            }
        }
    };

    let client = LlmClient::from_provider(provider, &model);
    let system_prompt = format!(
        "你是 NAS {domain} 领域专家。基于以下系统数据,用自然语言回答用户问题。\n\
         要点:\n\
         - 基于数据分析,不要臆测\n\
         - 发现异常要指出\n\
         - 给出具体建议\n\
         - 简洁,用中文"
    );
    let user_prompt = format!("用户问: {user_request}\n\n系统数据:\n{data}");
    Ok(client.chat(&system_prompt, &user_prompt).await?)
}

/// 从 models.toml 找域模型(execution_agent type 匹配 domain)
fn find_domain_model<'a>(domain: &str, library: &'a ModelLibrary) -> Option<(&'a crate::models::Provider, String)> {
    for ea in &library.execution_agents {
        if ea.agent_type == domain {
            if let Some(p) = library.find_provider(&ea.provider) {
                return Some((p, ea.model.clone()));
            }
        }
    }
    None
}

/// 回退:用 core 模型
fn find_core_model(library: &ModelLibrary) -> Option<(&crate::models::Provider, String)> {
    if library.core.provider.is_empty() || library.core.model.is_empty() {
        return None;
    }
    library.find_provider(&library.core.provider).map(|p| (p, library.core.model.clone()))
}
