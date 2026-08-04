//! backup-agent - 备份管理(预设执行体)。

use serde_json::{json, Value};
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct BackupAgent;

impl BackupAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "backup" => self.backup(args).await,
            "restore" => self.restore(args).await,
            "verify" => self.verify(args).await,
            "list_backups" => self.list_backups(args).await,
            "backup_status" => self.backup_status(args).await,
            "schedule_backup" => self.schedule_backup(args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    async fn backup(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest"); }
        run_cli("rsync", &["-av", "--delete", src, dest], &format!("备份 {src} -> {dest}")).await
    }

    async fn restore(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest"); }
        run_cli("rsync", &["-av", src, dest], &format!("恢复 {src} -> {dest}")).await
    }

    async fn verify(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest"); }
        run_cli("rsync", &["-avn", src, dest], &format!("校验 {src} vs {dest}")).await
    }

    async fn list_backups(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/usb");
        run_cli("ls", &["-la", "--time-style=long-iso", path], &format!("列备份 {path}")).await
    }

    async fn backup_status(&self, args: &Value) -> ActionResult {
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("/usb");
        let ls = tokio::process::Command::new("ls").args(["-la", "--time-style=long-iso", dest]).output().await;
        let listings = match ls { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(), _ => "无法读取".to_string() };
        let df = tokio::process::Command::new("df").args(["-h", dest]).output().await;
        let disk = match df { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().nth(1).unwrap_or("").to_string(), _ => "无法读取".to_string() };
        ActionResult::ok(json!({"backup_dir": dest, "listings": listings, "disk_space": disk}), format!("备份状态({dest})"))
    }

    async fn schedule_backup(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        let schedule = args.get("schedule").and_then(|v| v.as_str()).unwrap_or("0 2 * * 0");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest"); }
        let cron_line = format!("{schedule} root rsync -av --delete {} {} # aaos-backup\n", src, dest);
        match std::fs::write("/etc/cron.d/aaos-backup", &cron_line) {
            Ok(_) => ActionResult::ok(json!({"schedule": schedule, "src": src, "dest": dest}), format!("定时备份: {schedule}")),
            Err(e) => ActionResult::err(format!("写入 cron 失败: {e}")),
        }
    }

    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("/usb");
        let mut data = json!({});
        if let Ok(o) = tokio::process::Command::new("ls").args(["-la", dest]).output().await {
            data["backups"] = Value::String(String::from_utf8_lossy(&o.stdout).to_string());
        }
        if let Ok(o) = tokio::process::Command::new("df").args(["-h", dest]).output().await {
            data["disk_space"] = Value::String(String::from_utf8_lossy(&o.stdout).to_string());
        }
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("backup", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(json!({"analysis": analysis, "raw_data": data}), "备份智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}

async fn run_cli(cmd: &str, args: &[&str], label: &str) -> ActionResult {
    match tokio::process::Command::new(cmd).args(args).output().await {
        Ok(o) if o.status.success() => ActionResult::ok(json!({"output": String::from_utf8_lossy(&o.stdout).to_string()}), format!("{label} 成功")),
        Ok(o) => ActionResult::err(format!("{label} 失败: {}", String::from_utf8_lossy(&o.stderr))),
        Err(_) => ActionResult::err(format!("{label}: {cmd} 不可用")),
    }
}
