//! system-agent - 系统维护(预设执行体)。
//!
//! 域:磁盘 SMART 健康 / 系统信息 / 磁盘空间。
//! 预设流程:每个 action 映射到固定 OMV RPC 调用,执行后返回结构化 ActionResult。

use serde_json::{json, Value};

use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;
use crate::omv::{self, OmvCall};

pub struct SystemAgent;

impl SystemAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "check_smart" => self.check_smart().await,
            "system_info" => self.system_info().await,
            "list_disks" => self.list_disks().await,
            "list_filesystems" => self.list_filesystems().await,
            "list_users" => self.list_users().await,
            "disk_usage" => self.disk_usage().await,
            "check_state" => self.check_state(args).await,
            "apply_config" => self.apply_config().await,
            "clean_caches" => self.clean_caches().await,
            "check_updates" => self.check_updates().await,
            "network_info" => self.network_info().await,
            "reboot_system" => self.reboot_system().await,
            _ => self.smart_execute(action, args).await,
        }
    }

    /// SMART 设备列表(含健康/温度)
    async fn check_smart(&self) -> ActionResult {
        let call = OmvCall::new(
            "Smart",
            "getList",
            json!({"start": 0, "limit": 50}),
            "查 SMART 设备列表",
        );
        run_omv(call, "SMART 设备").await
    }

    /// 系统信息(CPU/内存/uptime/版本)
    async fn system_info(&self) -> ActionResult {
        let call = OmvCall::new("System", "getInformation", json!({}), "查系统信息");
        run_omv(call, "系统信息").await
    }

    /// 列出磁盘(DiskMgmt::enumerateDevices)
    async fn list_disks(&self) -> ActionResult {
        let call = OmvCall::new("DiskMgmt", "enumerateDevices", json!({}), "列磁盘");
        run_omv(call, "磁盘列表").await
    }

    /// 列出文件系统(FileSystemMgmt::enumerateFilesystems)
    async fn list_filesystems(&self) -> ActionResult {
        let call = OmvCall::new("FileSystemMgmt", "enumerateFilesystems", json!({}), "列文件系统");
        run_omv(call, "文件系统列表").await
    }

    /// 列出用户(UserMgmt::enumerateUsers)
    async fn list_users(&self) -> ActionResult {
        let call = OmvCall::new("UserMgmt", "enumerateUsers", json!({}), "列用户");
        run_omv(call, "用户列表").await
    }

    /// 磁盘/内存空间(System::getInformation 含内存;磁盘空间后续补 df)
    async fn disk_usage(&self) -> ActionResult {
        let call = OmvCall::new("System", "getInformation", json!({}), "查空间使用");
        run_omv(call, "空间使用").await
    }

    /// L3 实时状态查询:CPU/内存/SMART/容器(Core 的 check_state 工具经此执行)
    async fn check_state(&self, args: &Value) -> ActionResult {
        let area = args.get("area").and_then(|v| v.as_str()).unwrap_or("all");
        let mut results = Vec::new();

        if area == "all" || area == "disk" {
            let call = OmvCall::new("System", "getInformation", json!({}), "查系统状态");
            if let Ok(r) = omv::execute(&call).await {
                if r.success {
                    if let Ok(v) = serde_json::from_str::<Value>(&r.stdout) {
                        results.push(json!({
                            "area": "system",
                            "cpu_utilization": v.get("cpuUtilization"),
                            "mem_available": v.get("memAvailable"),
                            "uptime": v.get("uptime"),
                            "configDirty": v.get("configDirty")
                        }));
                    }
                }
            }
        }

        if area == "all" || area == "smart" {
            let call = OmvCall::new("Smart", "getList", json!({"start":0,"limit":50}), "查SMART");
            if let Ok(r) = omv::execute(&call).await {
                if r.success {
                    let data: Value = serde_json::from_str(&r.stdout).unwrap_or(json!([]));
                    results.push(json!({"area": "smart", "devices": data}));
                }
            }
        }

        if area == "all" || area == "containers" {
            match tokio::process::Command::new("podman")
                .args(["ps", "--format", "{{.Names}} {{.Status}}"])
                .output().await
            {
                Ok(o) if o.status.success() => {
                    let c = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    results.push(json!({"area": "containers", "running": if c.is_empty() {"无"} else {&c}}));
                }
                _ => results.push(json!({"area": "containers", "error": "podman不可用"})),
            }
        }

        ActionResult::ok(json!(results), format!("状态查询({area})完成"))
    }

    /// 应用 OMV 配置变更(Config::applyChangesBg)
    async fn apply_config(&self) -> ActionResult {
        match omv::apply_changes().await {
            Ok(r) => ActionResult::ok(serde_json::from_str(&r.stdout).unwrap_or(serde_json::Value::String(r.stdout.clone())), "配置已应用"),
            Err(e) => ActionResult::err(format!("应用配置失败: {e:#}")),
        }
    }

    /// 清理缓存(apt/tmp/旧日志)
    async fn clean_caches(&self) -> ActionResult {
        let mut results = Vec::new();
        let r = tokio::process::Command::new("apt-get").args(["clean"]).output().await;
        results.push(json!({"action": "apt_clean", "success": r.as_ref().map(|o| o.status.success()).unwrap_or(false)}));
        let r = tokio::process::Command::new("apt-get").args(["autoremove", "-y"]).output().await;
        results.push(json!({"action": "apt_autoremove", "success": r.as_ref().map(|o| o.status.success()).unwrap_or(false)}));
        let r = tokio::process::Command::new("bash").args(["-c", "find /tmp -type f -atime +7 -delete 2>/dev/null; echo done"]).output().await;
        results.push(json!({"action": "clean_tmp_7d", "success": r.as_ref().map(|o| o.status.success()).unwrap_or(false)}));
        let r = tokio::process::Command::new("bash").args(["-c", "find /var/log -name '*.gz' -mtime +30 -delete 2>/dev/null; journalctl --vacuum-time=30d 2>/dev/null; echo done"]).output().await;
        results.push(json!({"action": "clean_old_logs", "success": r.as_ref().map(|o| o.status.success()).unwrap_or(false)}));
        ActionResult::ok(json!(results), "缓存清理完成(apt+tmp+日志)")
    }

    /// 检查系统更新
    async fn check_updates(&self) -> ActionResult {
        let _ = tokio::process::Command::new("apt-get").args(["update", "-qq"]).output().await;
        match tokio::process::Command::new("apt").args(["list", "--upgradable"]).output().await {
            Ok(o) => {
                let list = String::from_utf8_lossy(&o.stdout).to_string();
                let count = list.lines().filter(|l| !l.is_empty() && !l.starts_with("Listing")).count();
                ActionResult::ok(json!({"upgradable": count, "packages": list}), format!("{count} 个可升级包"))
            }
            _ => ActionResult::err("检查更新失败"),
        }
    }

    /// 网络信息
    async fn network_info(&self) -> ActionResult {
        match tokio::process::Command::new("ip").args(["-j", "addr"]).output().await {
            Ok(o) if o.status.success() => {
                let data: Value = serde_json::from_str(&String::from_utf8_lossy(&o.stdout)).unwrap_or(Value::Null);
                ActionResult::ok(data, "网络接口信息")
            }
            _ => ActionResult::err("网络信息获取失败"),
        }
    }

    /// 重启系统(破坏性)
    async fn reboot_system(&self) -> ActionResult {
        ActionResult::ok(json!({"action": "reboot", "note": "需要确认后执行"}), "重启指令已准备,需确认")
    }

    /// LLM 智能分析:收集系统数据 -> 域 LLM 分析 -> 返回智能结果
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        // 从 args 取用户原始请求(Core 传过来的)
        let user_request = args.get("input").and_then(|v| v.as_str())
            .unwrap_or(action);

        // 收集域数据
        let mut data = json!({});

        // 系统信息
        let sys_call = OmvCall::new("System", "getInformation", json!({}), "查系统信息");
        if let Ok(r) = omv::execute(&sys_call).await {
            if r.success {
                data["system"] = serde_json::from_str(&r.stdout).unwrap_or(Value::String(r.stdout.clone()));
            }
        }

        // SMART
        let smart_call = OmvCall::new("Smart", "getList", json!({"start":0,"limit":50}), "查SMART");
        if let Ok(r) = omv::execute(&smart_call).await {
            if r.success {
                data["smart"] = serde_json::from_str(&r.stdout).unwrap_or(Value::Null);
            }
        }

        // journalctl 最近 50 行
        if let Ok(o) = tokio::process::Command::new("journalctl")
            .args(["-n", "50", "--no-pager", "-p", "err"])
            .output().await
        {
            data["recent_errors"] = Value::String(String::from_utf8_lossy(&o.stdout).to_string());
        }

        // 调域 LLM 分析
        let library = match ModelLibrary::load() {
            Ok(l) => l,
            Err(e) => return ActionResult::err(format!("模型库加载失败: {e}")),
        };

        match llm_helper::smart_analyze("system", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(
                json!({"analysis": analysis, "raw_data": data}),
                "系统智能分析完成"
            ),
            Err(e) => ActionResult::err(format!("LLM 分析失败: {e:#}")),
        }
    }
}

/// 跑一个 OmvCall,把 omv-rpc 的 JSON stdout 解析成 ActionResult.data。
async fn run_omv(call: OmvCall, label: &str) -> ActionResult {
    match omv::execute(&call).await {
        Ok(r) if r.success => {
            let data: Value = serde_json::from_str(&r.stdout).unwrap_or(Value::String(r.stdout.clone()));
            ActionResult::ok(data, format!("{label} 查询成功"))
        }
        Ok(r) => ActionResult::err(format!("{label} 查询失败: {}", r.stderr.chars().take(200).collect::<String>())),
        Err(e) => ActionResult::err(format!("{label} 查询出错: {e:#}")),
    }
}
