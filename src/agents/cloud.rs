//! cloud-agent - 云存储管理(预设执行体)。
//!
//! 域:百度网盘/阿里云盘/天翼云盘等(经 Alist REST API)。
//! Alist 容器提供统一 REST 接口,agent 调它操作网盘文件。

use serde_json::{json, Value};
use std::time::Duration;
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct CloudAgent;

impl CloudAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "list_cloud" => self.list_cloud(args).await,
            "download_from_cloud" => self.download_from_cloud(args).await,
            "upload_to_cloud" => self.upload_to_cloud(args).await,
            "cloud_info" => self.cloud_info().await,
            _ => self.smart_execute(action, args).await,
        }
    }

    /// 获取 Alist token
    async fn get_token(&self) -> Result<String, String> {
        let url = std::env::var("ALIST_URL").unwrap_or_default();
        let user = std::env::var("ALIST_USER").unwrap_or_default();
        let pass = std::env::var("ALIST_PASS").unwrap_or_default();
        if url.is_empty() {
            return Err("未配置 ALIST_URL 环境变量".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(format!("{}/api/auth/login", url))
            .json(&json!({"username": user, "password": pass}))
            .send()
            .await
            .map_err(|e| format!("Alist 登录请求失败: {e}"))?;
        let body: Value = resp.json().await.map_err(|e| format!("解析失败: {e}"))?;
        body.get("data")
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "Alist 登录失败(检查 ALIST_USER/PASS)".into())
    }

    /// Alist POST 请求(带 token)
    async fn alist_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let url = std::env::var("ALIST_URL").unwrap_or_default();
        let token = self.get_token().await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(format!("{}/api/{}", url, path))
            .header("Authorization", &token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Alist 请求失败: {e}"))?;
        let data: Value = resp.json().await.map_err(|e| format!("解析失败: {e}"))?;
        if data.get("code").and_then(|c| c.as_i64()) != Some(200) {
            let msg = data.get("message").and_then(|m| m.as_str()).unwrap_or("未知错误");
            return Err(format!("Alist 返回错误: {msg}"));
        }
        Ok(data)
    }

    /// 列网盘文件
    async fn list_cloud(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        match self.alist_post("fs/list", &json!({"path": path, "page": 1, "per_page": 50})).await {
            Ok(data) => {
                let content = data.get("data").and_then(|d| d.get("content")).cloned().unwrap_or(json!([]));
                let count = content.as_array().map(|a| a.len()).unwrap_or(0);
                ActionResult::ok(content, format!("网盘 {path}: {count} 项"))
            }
            Err(e) => ActionResult::err(e),
        }
    }

    /// 从网盘下载到 NAS
    async fn download_from_cloud(&self, args: &Value) -> ActionResult {
        let remote_path = args.get("remote_path").and_then(|v| v.as_str()).unwrap_or("");
        let local_path = args.get("local_path").and_then(|v| v.as_str()).unwrap_or("/srv/downloads");
        if remote_path.is_empty() {
            return ActionResult::err("缺少 remote_path 参数(网盘文件路径)");
        }
        let url = std::env::var("ALIST_URL").unwrap_or_default();
        // Alist 下载 URL: /d/<path>(需要 token 或公开链接)
        let download_url = format!("{}/d{}", url, remote_path);
        // 用 curl 下载(Alist 的 /d/ 端点支持直接下载)
        let filename = remote_path.rsplit('/').next().unwrap_or("download");
        let dest = if local_path.ends_with('/') {
            format!("{local_path}{filename}")
        } else {
            format!("{local_path}/{filename}")
        };
        match tokio::process::Command::new("curl")
            .args(["-L", "-o", &dest, &download_url])
            .output()
            .await
        {
            Ok(o) if o.status.success() => ActionResult::ok(
                json!({"url": download_url, "saved_to": dest}),
                format!("下载完成: {remote_path} -> {dest}")
            ),
            Ok(o) => ActionResult::err(format!("下载失败: {}", String::from_utf8_lossy(&o.stderr).chars().take(300).collect::<String>())),
            Err(e) => ActionResult::err(format!("curl 执行失败: {e}")),
        }
    }

    /// 从 NAS 上传到网盘
    async fn upload_to_cloud(&self, args: &Value) -> ActionResult {
        let local_path = args.get("local_path").and_then(|v| v.as_str()).unwrap_or("");
        let remote_path = args.get("remote_path").and_then(|v| v.as_str()).unwrap_or("/");
        if local_path.is_empty() {
            return ActionResult::err("缺少 local_path 参数(NAS 本地文件路径)");
        }
        let url = std::env::var("ALIST_URL").unwrap_or_default();
        let token = match self.get_token().await {
            Ok(t) => t,
            Err(e) => return ActionResult::err(e),
        };
        let filename = local_path.rsplit('/').next().unwrap_or("upload");
        let dest_path = if remote_path.ends_with('/') {
            format!("{remote_path}{filename}")
        } else {
            format!("{remote_path}/{filename}")
        };
        // Alist 上传 API: PUT /api/fs/put
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
        {
            Ok(c) => c,
            Err(e) => return ActionResult::err(format!("HTTP 客户端构建失败: {e}")),
        };
        // 读文件
        let file_data = match tokio::fs::read(local_path).await {
            Ok(d) => d,
            Err(e) => return ActionResult::err(format!("读文件失败: {e}")),
        };
        let upload_url = format!("{}/api/fs/put", url);
        match client
            .put(&upload_url)
            .header("Authorization", &token)
            .header("File-Path", &dest_path)
            .header("As-Task", "false")
            .body(file_data)
            .send()
            .await
        {
            Ok(resp) => {
                let body: Value = resp.json().await.unwrap_or(json!({}));
                if body.get("code").and_then(|c| c.as_i64()) == Some(200) {
                    ActionResult::ok(json!({"local": local_path, "remote": dest_path}),
                        format!("上传完成: {local_path} -> 网盘{dest_path}"))
                } else {
                    let msg = body.get("message").and_then(|m| m.as_str()).unwrap_or("未知错误");
                    ActionResult::err(format!("上传失败: {msg}"))
                }
            }
            Err(e) => ActionResult::err(format!("上传请求失败: {e}")),
        }
    }

    /// 网盘信息(已配置的存储列表)
    async fn cloud_info(&self) -> ActionResult {
        match self.alist_post("admin/storage/list", &json!({"page": 1, "per_page": 50})).await {
            Ok(data) => {
                let storages = data.get("data").and_then(|d| d.get("content")).cloned().unwrap_or(json!([]));
                let count = storages.as_array().map(|a| a.len()).unwrap_or(0);
                ActionResult::ok(storages, format!("已配置 {count} 个云存储"))
            }
            Err(e) => ActionResult::err(format!("查询存储列表失败: {e}")),
        }
    }

    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let mut data = json!({});
        let info = self.cloud_info().await;
        if info.success {
            data["cloud_info"] = info.data;
        }
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("cloud", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(json!({"analysis": analysis}), "云存储智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}
