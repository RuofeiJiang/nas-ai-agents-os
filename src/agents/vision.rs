//! vision-agent - 相册管理(预设执行体)。
//!
//! 域:照片扫描/视觉打标签/批量打标签/搜索。用 CLI(find) + vision 模型 API + SQLite 标签库。

use serde_json::{json, Value};
use std::time::Duration;
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct VisionAgent;

impl VisionAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "scan_photos" => self.scan_photos(args).await,
            "tag_photo" => self.tag_photo(args).await,
            "tag_all_photos" => self.tag_all_photos(args).await,
            "search_photos" => self.search_photos(args).await,
            "build_album" => self.build_album(args).await,
            "photo_stats" => self.photo_stats().await,
            _ => self.smart_execute(action, args).await,
        }
    }

    /// 扫描目录下的照片文件
    async fn scan_photos(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/srv");
        match tokio::process::Command::new("find")
            .args([path, "-maxdepth", "3", "-type", "f",
                   "(", "-name", "*.jpg", "-o", "-name", "*.jpeg", "-o", "-name", "*.png", "-o", "-name", "*.heic", ")"])
            .output().await
        {
            Ok(o) if o.status.success() => {
                let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
                let photos: Vec<&str> = stdout_str.lines().collect();
                ActionResult::ok(json!({"count": photos.len(), "photos": photos}),
                    format!("扫描到 {} 张照片", photos.len()))
            }
            _ => ActionResult::err("扫描失败"),
        }
    }

    /// 给单张照片打标签(调 vision 模型)
    async fn tag_photo(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() {
            return ActionResult::err("缺少 path 参数");
        }
        // 读图片 base64
        let img_data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => return ActionResult::err(format!("读图片失败: {e}")),
        };
        let b64 = base64_encode(&img_data);
        let ext = path.rsplit('.').next().unwrap_or("jpg").to_lowercase();
        let mime = match ext.as_str() { "png" => "image/png", "heic" => "image/heic", _ => "image/jpeg" };

        // 从模型库取 vision 模型
        let lib = match ModelLibrary::load() {
            Ok(l) => l,
            Err(e) => return ActionResult::err(format!("模型库加载失败: {e}")),
        };
        // 找 vision-capable 模型
        let (provider, model) = find_vision_model(&lib);
        let (provider, model) = match (provider, model) {
            (Some(p), Some(m)) => (p, m),
            _ => return ActionResult::err("未配置 vision 模型(在 models.toml 里配 capabilities含 vision 的模型)"),
        };

        let api_key = if provider.api_key_env.is_empty() {
            None
        } else {
            std::env::var(&provider.api_key_env).ok().filter(|s| !s.is_empty())
        };
        let url = format!("{}/chat/completions", provider.base_url.trim_end_matches('/'));
        let body = json!({
            "model": model,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "给这张照片打标签。返回 JSON: {\"tags\":[\"标签1\",\"标签2\"],\"scene\":\"场景描述\",\"objects\":[\"物体1\"]}"},
                {"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, b64)}}
            ]}],
            "temperature": 0.0
        });

        let client = reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap();
        let mut req = client.post(&url).json(&body);
        if let Some(key) = &api_key { req = req.bearer_auth(key); }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                let json: Value = resp.json().await.unwrap_or(json!({}));
                let content = json.get("choices").and_then(|c| c.get(0))
                    .and_then(|c| c.get("message")).and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str()).unwrap_or("");
                let tags: Value = serde_json::from_str(content).unwrap_or(Value::String(content.to_string()));
                ActionResult::ok(tags, format!("照片 {path} 打标签完成"))
            }
            Ok(resp) => ActionResult::err(format!("vision API 返回 {}", resp.status())),
            Err(e) => ActionResult::err(format!("vision API 调用失败: {e}")),
        }
    }

    /// 批量打标签(scan + 逐张 tag,存 JSON 文件)
    async fn tag_all_photos(&self, args: &Value) -> ActionResult {
        // 先 scan
        let scan = self.scan_photos(args).await;
        if !scan.success { return scan; }
        let photos = scan.data.get("photos").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        let mut tagged = 0;
        let mut results = Vec::new();
        for photo in photos.iter().take(20) { // MVP 限制 20 张
            if let Some(path) = photo.as_str() {
                let tag_args = json!({"path": path});
                let r = self.tag_photo(&tag_args).await;
                if r.success {
                    tagged += 1;
                    results.push(json!({"path": path, "tags": r.data}));
                }
            }
        }
        // 存到 JSON 文件(后续换 SQLite)
        let _ = std::fs::write("/var/lib/aaos/photo-tags.json", serde_json::to_string_pretty(&results).unwrap_or_default());
        ActionResult::ok(json!({"tagged": tagged, "total": photos.len()}),
            format!("批量打标签完成: {tagged}/{} 张", photos.len()))
    }

    /// 搜索照片(从 JSON 标签库查)
    async fn search_photos(&self, args: &Value) -> ActionResult {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let tags_text = std::fs::read_to_string("/var/lib/aaos/photo-tags.json").unwrap_or_default();
        let tags: Vec<Value> = serde_json::from_str(&tags_text).unwrap_or_default();
        let matches: Vec<&Value> = tags.iter().filter(|t| t.to_string().to_lowercase().contains(&query)).collect();
        ActionResult::ok(json!({"query": query, "matches": matches.len(), "results": matches}),
            format!("搜索 \"{query}\": {} 张", matches.len()))
    }

    /// 智能相册:按主题从标签库筛选
    async fn build_album(&self, args: &Value) -> ActionResult {
        let theme = args.get("theme").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let tags_text = std::fs::read_to_string("/var/lib/aaos/photo-tags.json").unwrap_or_default();
        let tags: Vec<Value> = serde_json::from_str(&tags_text).unwrap_or_default();
        let album: Vec<&Value> = tags.iter().filter(|t| t.to_string().to_lowercase().contains(&theme)).collect();
        ActionResult::ok(json!({"theme": theme, "photos": album.len(), "results": album}),
            format!("智能相册 \"{theme}\": {} 张", album.len()))
    }

    /// 照片标签统计
    async fn photo_stats(&self) -> ActionResult {
        let tags_text = std::fs::read_to_string("/var/lib/aaos/photo-tags.json").unwrap_or_default();
        let tags: Vec<Value> = serde_json::from_str(&tags_text).unwrap_or_default();
        let mut tag_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for t in &tags {
            if let Some(tags_arr) = t.get("tags").and_then(|x| x.as_array()) {
                for tag in tags_arr {
                    if let Some(s) = tag.as_str() {
                        *tag_counts.entry(s.to_string()).or_default() += 1;
                    }
                }
            }
        }
        let mut sorted: Vec<(String, usize)> = tag_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        ActionResult::ok(json!({
            "total_photos": tags.len(),
            "total_tags": sorted.len(),
            "top_tags": sorted.iter().take(20).map(|(t, c)| json!({"tag": t, "count": c})).collect::<Vec<_>>()
        }), format!("照片统计: {} 张已标记, {} 种标签", tags.len(), sorted.len()))
    }
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let data = serde_json::json!({});
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("vision", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(serde_json::json!({"analysis": analysis}), "智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}

/// 从模型库找 vision-capable 模型
fn find_vision_model(lib: &ModelLibrary) -> (Option<&crate::models::Provider>, Option<String>) {
    for p in &lib.providers {
        for m in &p.models {
            if m.capabilities.iter().any(|c| c == "vision") {
                return (Some(p), Some(m.id.clone()));
            }
        }
    }
    (None, None)
}
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        result.push(CHARS[(b[0] >> 2) as usize] as char);
        result.push(CHARS[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char);
        } else { result.push('='); }
        if chunk.len() > 2 {
            result.push(CHARS[(b[2] & 0x3f) as usize] as char);
        } else { result.push('='); }
    }
    result
}
