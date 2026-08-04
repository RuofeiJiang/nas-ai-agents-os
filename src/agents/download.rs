//! download-agent - 下载管理(预设执行体)。
//!
//! 域:BT/PT 下载/网盘/自动整理入库。需 qBittorrent/Transmission(后续接入)。

use serde_json::{json, Value};
use std::time::Duration;
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct DownloadAgent;

impl DownloadAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "add_torrent" => self.add_torrent(args).await,
            "list_downloads" => self.list_downloads().await,
            "move_to_library" => self.move_to_library(args).await,
            "pause_torrent" => self.torrent_op("pause", args).await,
            "resume_torrent" => self.torrent_op("resume", args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    async fn add_torrent(&self, args: &Value) -> ActionResult {
        let magnet = args.get("magnet").and_then(|v| v.as_str()).unwrap_or("");
        if magnet.is_empty() { return ActionResult::err("缺少 magnet 参数"); }
        let host = std::env::var("QBIT_HOST").unwrap_or_default();
        if host.is_empty() {
            return ActionResult::err("需配置 QBIT_HOST/QBIT_USER/QBIT_PASS 环境变量(qBittorrent Web API 地址)");
        }
        let user = std::env::var("QBIT_USER").unwrap_or_default();
        let pass = std::env::var("QBIT_PASS").unwrap_or_default();
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
            Ok(c) => c, Err(_) => return ActionResult::err("HTTP 客户端构建失败"),
        };
        // 登录
        let login_url = format!("{}/api/v2/auth/login", host);
        let login = client.post(&login_url).form(&[("username", &user), ("password", &pass)]).send().await;
        if login.is_err() { return ActionResult::err(format!("qBittorrent 登录失败: {}", host)); }
        // 添加 torrent
        let add_url = format!("{}/api/v2/torrents/add", host);
        match client.post(&add_url).form(&[("urls", magnet)]).send().await {
            Ok(r) if r.status().is_success() => ActionResult::ok(json!({"magnet": &magnet[..magnet.len().min(60)]}), "torrent 添加成功"),
            Ok(r) => ActionResult::err(format!("qBittorrent 添加失败: {}", r.status())),
            Err(e) => ActionResult::err(format!("qBittorrent API 错误: {e}")),
        }
    }

    async fn list_downloads(&self) -> ActionResult {
        let host = std::env::var("QBIT_HOST").unwrap_or_default();
        if host.is_empty() {
            return ActionResult::err("需配置 QBIT_HOST(qBittorrent Web API 地址)");
        }
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
            Ok(c) => c, Err(_) => return ActionResult::err("HTTP 客户端构建失败"),
        };
        let url = format!("{}/api/v2/torrents/info", host);
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                let data: Value = r.json().await.unwrap_or(json!([]));
                ActionResult::ok(data, "下载列表")
            }
            Ok(r) => ActionResult::err(format!("qBittorrent 返回 {}", r.status())),
            Err(e) => ActionResult::err(format!("qBittorrent API 错误: {e}")),
        }
    }

    async fn move_to_library(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest 参数"); }
        match tokio::process::Command::new("mv").args([src, dest]).output().await {
            Ok(o) if o.status.success() => ActionResult::ok(json!({"moved": format!("{src} -> {dest}")}), "文件移动成功"),
            Ok(o) => ActionResult::err(format!("移动失败: {}", String::from_utf8_lossy(&o.stderr))),
            Err(_) => ActionResult::err("mv 命令失败"),
        }
    }

    /// 暂停/恢复 torrent
    async fn torrent_op(&self, op: &str, args: &Value) -> ActionResult {
        let hash = args.get("hash").and_then(|v| v.as_str()).unwrap_or("");
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if hash.is_empty() && name.is_empty() {
            return ActionResult::err("缺少 hash 或 name 参数");
        }
        let host = std::env::var("QBIT_HOST").unwrap_or_default();
        if host.is_empty() {
            return ActionResult::err("需配置 QBIT_HOST");
        }
        let user = std::env::var("QBIT_USER").unwrap_or_default();
        let pass = std::env::var("QBIT_PASS").unwrap_or_default();
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
            Ok(c) => c, Err(_) => return ActionResult::err("HTTP 客户端构建失败"),
        };
        // 登录
        let login_url = format!("{}/api/v2/auth/login", host);
        let _ = client.post(&login_url).form(&[("username", &user), ("password", &pass)]).send().await;
        // 暂停/恢复
        let api_path = if op == "pause" { "pause" } else { "resume" };
        let url = format!("{}/api/v2/torrents/{}", host, api_path);
        let param_name = if hash.is_empty() { "hashes" } else { "hashes" };
        let param_val = if hash.is_empty() { name } else { hash };
        match client.post(&url).form(&[(param_name, param_val)]).send().await {
            Ok(r) if r.status().is_success() => ActionResult::ok(json!({"op": op, "target": param_val}), format!("torrent {op} 成功")),
            Ok(r) => ActionResult::err(format!("qBittorrent {op} 失败: {}", r.status())),
            Err(e) => ActionResult::err(format!("qBittorrent API 错误: {e}")),
        }
    }
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let mut data = serde_json::json!({});
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("download", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(serde_json::json!({"analysis": analysis}), "智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}
