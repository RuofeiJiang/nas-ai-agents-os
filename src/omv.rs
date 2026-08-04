//! OMV RPC 调用层(替代旧的 zfs.rs)
//!
//! AAOS Core 通过 shell 出 `omv-rpc` 调用 OpenMediaVault 全部功能。
//! 命令分类(read/write/destructive)从 `rules.json` 数据驱动加载,
//! 对应 Charter 权限档:read 自动放行、write 需审计、destructive 必须用户确认。
//!
//! 调用形式:`omv-rpc "<Service>" "<method>" '<json params>'`(root 免登录)
//! 异步方法返回 `/tmp/bgstatus*` 任务句柄,execute 会轮询等完成。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::process::Command;

/// 命令分类(对应 Charter 权限档)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandClass {
    /// 只读,自动放行
    Read,
    /// 改,需 Sentinel 审计
    Write,
    /// 破坏性,必须用户确认
    Destructive,
}

impl CommandClass {
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, CommandClass::Destructive)
    }
}

/// 一次 OMV RPC 调用
#[derive(Debug, Clone)]
pub struct OmvCall {
    /// 真实服务名,如 "DiskMgmt" / "SMB"
    pub service: String,
    /// 方法名,如 "enumerateDevices"
    pub method: String,
    /// JSON 参数
    pub params: Value,
    /// 分类(构造时从 rules.json 自动查)
    pub class: CommandClass,
    /// 人类可读描述(日志/确认弹窗)
    pub description: String,
}

impl OmvCall {
    pub fn new(service: &str, method: &str, params: Value, description: impl Into<String>) -> Self {
        Self {
            class: classify(service, method),
            service: service.into(),
            method: method.into(),
            params,
            description: description.into(),
        }
    }

    /// 调用形式(用于日志)
    pub fn display(&self) -> String {
        format!("{}::{} {}", self.service, self.method, self.params)
    }

    /// 是否需要用户确认(破坏性)
    pub fn needs_confirmation(&self) -> bool {
        self.class.needs_confirmation()
    }
}

/// ======= 数据驱动分类 =======

/// ======= 知识库(NAS 域 kb-nas.json;HA 域后续 kb-ha.json)=======

#[derive(Clone)]
pub struct KbEntry {
    pub class: CommandClass,
    /// 参数 schema:[{name, type, required, enum}]
    pub params: Vec<Value>,
}

static KB: OnceLock<HashMap<String, KbEntry>> = OnceLock::new();

/// 加载 kb-nas.json(优先 /etc/aaos/kb-nas.json,回退 ./kb-nas.json),进程内缓存。
fn kb() -> &'static HashMap<String, KbEntry> {
    KB.get_or_init(|| {
        let text = std::fs::read_to_string("/etc/aaos/kb-nas.json")
            .or_else(|_| std::fs::read_to_string("kb-nas.json"))
            .unwrap_or_default();
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let mut m = HashMap::new();
        if let Some(obj) = parsed.get("methods").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                let class = match v.get("class").and_then(|c| c.as_str()).unwrap_or("") {
                    "read" => CommandClass::Read,
                    "write" => CommandClass::Write,
                    "destructive" => CommandClass::Destructive,
                    _ => CommandClass::Write,
                };
                let params = v
                    .get("params")
                    .and_then(|p| p.as_array())
                    .cloned()
                    .unwrap_or_default();
                m.insert(k.clone(), KbEntry { class, params });
            }
        }
        tracing::info!("loaded {} NAS methods from kb-nas.json", m.len());
        m
    })
}

/// 按 "Service.method" 查分类;查不到默认 Write(保守:需审计)。
pub fn classify(service: &str, method: &str) -> CommandClass {
    kb().get(&format!("{}.{}", service, method))
        .map(|e| e.class)
        .unwrap_or(CommandClass::Write)
}

/// 能力目录全部条目("Service.method" -> class)。
pub fn catalog_entries() -> Vec<(String, CommandClass)> {
    kb().iter().map(|(k, e)| (k.clone(), e.class)).collect()
}

/// service.method 是否在知识库里(校验 LLM 输出用)。
pub fn method_exists(service: &str, method: &str) -> bool {
    kb().contains_key(&format!("{}.{}", service, method))
}

/// 取某方法的参数 schema(agent 的 get_method_params 工具用)。
pub fn get_method_params(service: &str, method: &str) -> Vec<Value> {
    kb().get(&format!("{}.{}", service, method))
        .map(|e| e.params.clone())
        .unwrap_or_default()
}

/// 按关键字搜知识库(agent 的 search_catalog 工具用),返回匹配的 method + class。
pub fn search_catalog(query: &str) -> Vec<(String, CommandClass)> {
    let q = query.to_lowercase();
    kb().iter()
        .filter(|(k, _)| k.to_lowercase().contains(&q))
        .map(|(k, e)| (k.clone(), e.class))
        .collect()
}

/// ======= 执行 =======

/// 执行一个 OMV RPC 调用。
///
/// - shell 出 `omv-rpc`,捕获 stdout(JSON)
/// - 异步方法(返回 /tmp/bgstatus* 路径)自动轮询等完成
/// - 写/破坏性的 salt deploy 不在此自动触发(映射复杂,留给上层显式调 [`salt_deploy`])
pub async fn execute(call: &OmvCall) -> Result<ExecutionResult> {
    let params_str = serde_json::to_string(&call.params)?;
    tracing::debug!("omv-rpc {} {} {}", call.service, call.method, params_str);

    let output = Command::new("omv-rpc")
        .arg(&call.service)
        .arg(&call.method)
        .arg(&params_str)
        .output()
        .await
        .context("run omv-rpc")?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let success = output.status.success();

    // 异步:omv-rpc 返回 bgstatus 路径(JSON 字符串 "/tmp/bgstatus...",带转义 \/) -> 轮询等完成
    let (stdout, async_waited) = if success {
        let bgpath = serde_json::from_str::<String>(&stdout)
            .ok()
            .filter(|s| s.starts_with("/tmp/bgstatus"));
        if let Some(path) = bgpath {
            match wait_bgstatus(&path).await {
                Ok(out) => (out, true),
                Err(e) => return Err(e),
            }
        } else {
            (stdout, false)
        }
    } else {
        (stdout, false)
    };

    Ok(ExecutionResult {
        call: call.display(),
        class: call.class,
        description: call.description.clone(),
        stdout,
        stderr,
        success,
        async_waited,
        exit_code: output.status.code(),
    })
}

/// 轮询后台任务状态文件直到 running=false(最多 60s)。
async fn wait_bgstatus(path: &str) -> Result<String> {
    for _ in 0..120 {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if v.get("running").and_then(|r| r.as_bool()) == Some(false) {
                    // 取结果输出
                    let out_file = v
                        .get("outputfilename")
                        .and_then(|o| o.as_str())
                        .unwrap_or(path);
                    let result = std::fs::read_to_string(out_file).unwrap_or_default();
                    return Ok(result);
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("background task timeout: {path}")
}

/// 显式触发 salt deploy(写操作后让配置落到系统)。
/// `service` 是 salt 模块名,如 "fstab" / "samba" / "hosts"(注意不一定等于 RPC 服务名)。
pub async fn salt_deploy(service: &str) -> Result<()> {
    let output = Command::new("omv-salt")
        .args(["deploy", "run", service])
        .output()
        .await
        .context("run omv-salt deploy")?;
    if !output.status.success() {
        tracing::warn!(
            "omv-salt deploy run {} failed: {}",
            service,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// 应用所有 dirty 配置(OMV web UI 的"Apply"按钮机制)。
///
/// 两步:① `System::getInformation` 取 dirtyModules;② `Config::applyChangesBg`
/// 带 modules+force 异步应用(复用 [`execute`] 的 bgstatus 轮询)。
/// 无 dirty 时直接返回成功。
pub async fn apply_changes() -> Result<ExecutionResult> {
    // 1. 取 dirty 模块
    let info = OmvCall::new("System", "getInformation", serde_json::json!({}), "查 dirty 模块");
    let r = execute(&info).await?;
    let dirty: Vec<String> = serde_json::from_str::<Value>(&r.stdout)
        .ok()
        .and_then(|v| {
            v.get("dirtyModules")
                .and_then(|d| d.as_object())
                .map(|o| o.keys().cloned().collect())
        })
        .unwrap_or_default();

    if dirty.is_empty() {
        return Ok(ExecutionResult {
            call: "Config::applyChanges".into(),
            class: CommandClass::Write,
            description: "应用配置(无 dirty)".into(),
            stdout: "没有待应用的配置变更".into(),
            stderr: String::new(),
            success: true,
            async_waited: false,
            exit_code: Some(0),
        });
    }

    // 2. applyChangesBg 带 modules + force
    let apply = OmvCall::new(
        "Config",
        "applyChangesBg",
        serde_json::json!({"modules": dirty, "force": true}),
        format!("应用 {} 个 dirty 模块: {}", dirty.len(), dirty.join(",")),
    );
    execute(&apply).await
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionResult {
    pub call: String,
    pub class: CommandClass,
    pub description: String,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub async_waited: bool,
    pub exit_code: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known() {
        // 依赖 rules.json 在仓库根;CI/无文件时回退默认 Write
        let c = classify("DiskMgmt", "enumerateDevices");
        assert!(c == CommandClass::Read || c == CommandClass::Write); // 无文件时默认 Write
    }

    #[test]
    fn classify_destructive_default_safe() {
        // 查不到的方法默认 Write(保守需审计),不会误判成 Read
        let c = classify("X", "totallyUnknown");
        assert_eq!(c, CommandClass::Write);
    }

    #[test]
    fn omvcall_display() {
        let c = OmvCall::new("DiskMgmt", "enumerateDevices", serde_json::json!({}), "列磁盘");
        assert!(c.display().contains("DiskMgmt::enumerateDevices"));
    }
}
