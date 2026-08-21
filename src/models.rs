//! AAOS 模型库 -- 管理 provider + 模型(借鉴 cc-switch 的 provider 配置结构)。
//!
//! - 配置:`/etc/aaos/models.toml`(回退 `./models.toml`)
//! - 自动发现:provider 的 `GET {base_url}/models`(OpenAI 兼容)拉模型列表
//! - 启发式标注:按模型名猜 capabilities(text/vision)和 tags(strong/fast/cheap)
//!
//! provider 分类借 cc-switch ProviderCategory:official / aggregator / custom / ...

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 整个模型库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLibrary {
    /// 当前默认 provider id(借 cc-switch 的 current)
    pub current: String,
    #[serde(default)]
    pub providers: Vec<Provider>,
    /// Core Agent 绑定
    #[serde(default)]
    pub core: AgentBinding,
    /// Execution Agents(借 cc-switch 的 apps,换成 AAOS 角色分配)
    #[serde(default, rename = "execution_agent")]
    pub execution_agents: Vec<ExecutionAgent>,
    /// Sentinel 绑定(空=纯规则)
    #[serde(default)]
    pub sentinel: AgentBinding,
}

/// 一个 provider(借 cc-switch UniversalProvider)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// official / aggregator / custom / cloud_provider / ...
    #[serde(default)]
    pub category: String,
    /// 协议:anthropic / openai / ollama
    pub provider_type: String,
    pub base_url: String,
    /// 存 api key 的环境变量名(不直接存 key)
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub models: Vec<Model>,
    /// 是否加入故障转移队列(借 cc-switch in_failover_queue)
    #[serde(default)]
    pub in_failover_queue: bool,
}

/// 一个模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// text / vision / code(可多选)
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// strong / fast / cheap / balanced / local
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
}

/// Agent -> provider+model 绑定
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentBinding {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub fallback_provider: String,
    #[serde(default)]
    pub fallback_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAgent {
    #[serde(rename = "type")]
    pub agent_type: String,
    pub provider: String,
    pub model: String,
}

impl ModelLibrary {
    /// 从 /etc/aaos/models.toml(回退 ./models.toml)加载。
    pub fn load() -> Result<Self> {
        let text = std::fs::read_to_string("/etc/aaos/models.toml")
            .or_else(|_| std::fs::read_to_string("models.toml"))
            .context("models.toml not found (looked in /etc/aaos/ and ./)")?;
        Self::from_str(&text)
    }

    pub fn from_str(text: &str) -> Result<Self> {
        let lib: ModelLibrary = toml::from_str(text).context("parse models.toml")?;
        Ok(lib)
    }

    pub fn find_provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn find_model(&self, provider_id: &str, model_id: &str) -> Option<&Model> {
        self.find_provider(provider_id)?
            .models
            .iter()
            .find(|m| m.id == model_id)
    }
}

impl Provider {
    /// 发现模型。OpenAI provider 走 /models；Anthropic-compatible provider 不依赖发现端点，使用配置中的显式模型。
    pub async fn discover_models(&self) -> Result<Vec<Model>> {
        if self.provider_type.to_lowercase() == "anthropic" {
            return Ok(self.models.clone());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req = client.get(&url);
        if !self.api_key_env.is_empty() {
            if let Ok(key) = std::env::var(&self.api_key_env) {
                if !key.is_empty() {
                    req = req.bearer_auth(key);
                }
            }
        }
        let resp = req.send().await.context("GET /models")?;
        if !resp.status().is_success() {
            anyhow::bail!("{} /models returned {}", self.id, resp.status());
        }
        let body: ModelsResponse = resp.json().await.context("decode /models response")?;
        let models = body
            .data
            .into_iter()
            .map(|m| {
                let id = m.id;
                Model {
                    id: id.clone(),
                    capabilities: guess_capabilities(&id),
                    tags: guess_tags(&id),
                    context_window: None,
                }
            })
            .collect();
        Ok(models)
    }
}

/// OpenAI 兼容 /v1/models 响应
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelsResponseItem>,
}
#[derive(Debug, Deserialize)]
struct ModelsResponseItem {
    id: String,
}

/// 按模型名猜能力(text 必有;vision 按多模态模型名模式)
pub fn guess_capabilities(id: &str) -> Vec<String> {
    let l = id.to_lowercase();
    let mut caps = vec!["text".to_string()];
    let vision_patterns = [
        "gpt-4o", "gpt-4.1", "gemini", "claude-3", "claude-sonnet", "claude-opus",
        "qwen-vl", "qwen2-vl", "qwen2.5-vl", "llava", "minicpm-v", "cogvlm", "-vl",
        "vision", "llama3.2-vision", "minimax-m3",
    ];
    if vision_patterns.iter().any(|p| l.contains(p)) {
        caps.push("vision".to_string());
    }
    caps
}

/// 按模型名猜 tags(strong/fast/cheap/balanced)
pub fn guess_tags(id: &str) -> Vec<String> {
    let l = id.to_lowercase();
    let mut tags = vec![];
    let strong = ["sonnet", "opus", "gpt-4o", "gpt-4.1", "gpt-4", "claude-3.5",
                  "gemini-1.5-pro", "gemini-2", "deepseek-r1", "o1", "o3"];
    let fast = ["flash", "haiku", "mini", "nano", "small", "8b", "7b", "3b", "tiny", "highspeed"];
    if strong.iter().any(|p| l.contains(p)) {
        tags.push("strong".to_string());
    }
    if fast.iter().any(|p| l.contains(p)) {
        tags.push("fast".to_string());
        tags.push("cheap".to_string());
    }
    if tags.is_empty() {
        tags.push("balanced".to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_caps_text_only() {
        assert_eq!(guess_capabilities("deepseek-chat"), ["text"]);
        assert_eq!(guess_capabilities("gpt-3.5-turbo"), ["text"]);
    }

    #[test]
    fn guess_caps_vision() {
        let c = guess_capabilities("gpt-4o");
        assert!(c.contains(&"vision".to_string()));
        let c = guess_capabilities("qwen2.5-vl:7b");
        assert!(c.contains(&"vision".to_string()));
        let c = guess_capabilities("llava:13b");
        assert!(c.contains(&"vision".to_string()));
    }

    #[test]
    fn guess_minimax_capabilities_and_speed() {
        assert!(guess_capabilities("MiniMax-M3").contains(&"vision".to_string()));
        assert!(guess_tags("MiniMax-M2.7-highspeed").contains(&"fast".to_string()));
    }

    #[test]
    fn guess_tags_strong() {
        let t = guess_tags("claude-sonnet-5");
        assert!(t.contains(&"strong".to_string()));
    }

    #[test]
    fn guess_tags_fast_cheap() {
        let t = guess_tags("gemini-2.5-flash");
        assert!(t.contains(&"fast".to_string()));
        assert!(t.contains(&"cheap".to_string()));
    }

    #[test]
    fn guess_tags_balanced_default() {
        let t = guess_tags("some-unknown-model");
        assert_eq!(t, ["balanced"]);
    }

    #[test]
    fn load_sample_toml() {
        let toml = r#"
current = "p1"
[[providers]]
id = "p1"
name = "Test"
category = "aggregator"
provider_type = "openai"
base_url = "https://api.test/v1"
api_key_env = "TEST_KEY"

  [[providers.models]]
  id = "gpt-4o"
  capabilities = ["text", "vision"]
  tags = ["strong"]

[core]
provider = "p1"
model = "gpt-4o"

[[execution_agent]]
type = "text-gen"
provider = "p1"
model = "gpt-4o"
"#;
        let lib = ModelLibrary::from_str(toml).unwrap();
        assert_eq!(lib.current, "p1");
        assert_eq!(lib.providers.len(), 1);
        assert_eq!(lib.core.model, "gpt-4o");
        assert_eq!(lib.execution_agents.len(), 1);
        let m = lib.find_model("p1", "gpt-4o").unwrap();
        assert!(m.capabilities.contains(&"vision".to_string()));
    }
}
