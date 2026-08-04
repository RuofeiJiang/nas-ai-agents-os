//! media-agent - 影音管理(预设执行体)。

use serde_json::{json, Value};
use std::time::Duration;
use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct MediaAgent;

impl MediaAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "scrape_metadata" => self.scrape_metadata(args).await,
            "find_subtitle" => self.find_subtitle(args).await,
            "transcode" => self.transcode(args).await,
            "media_library_scan" => self.media_library_scan(args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    async fn scrape_metadata(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() { return ActionResult::err("缺少 path 参数"); }
        let filename = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let year = filename.matches(|c: char| c.is_ascii_digit()).collect::<String>();
        let year = if year.len() >= 4 { year[..4].to_string() } else { String::new() };
        let title = filename.replace(&format!(".{year}"), "").replace('.', " ").trim().to_string();
        if let Ok(key) = std::env::var("TMDB_API_KEY") {
            if !key.is_empty() {
                let url = format!("https://api.themoviedb.org/3/search/multi?api_key={}&query={}&language=zh-CN", key, urlenc(&title));
                if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(10)).build() {
                    if let Ok(resp) = client.get(&url).send().await {
                        if resp.status().is_success() {
                            if let Ok(json) = resp.json::<Value>().await {
                                if let Some(r) = json.get("results").and_then(|r| r.get(0)) {
                                    return ActionResult::ok(json!({
                                        "title": r.get("title").or_else(|| r.get("name")).and_then(|t| t.as_str()).unwrap_or(&title),
                                        "year": r.get("release_date").or_else(|| r.get("first_air_date")).and_then(|d| d.as_str()).map(|s| s.get(..4).unwrap_or("").to_string()).unwrap_or(year),
                                        "overview": r.get("overview").and_then(|o| o.as_str()).unwrap_or(""),
                                        "source": "TMDB"
                                    }), format!("TMDB: {title}"));
                                }
                            }
                        }
                    }
                }
            }
        }
        ActionResult::ok(json!({"title": title, "year": year, "source": "filename"}), format!("文件名解析: {title}"))
    }

    async fn find_subtitle(&self, _args: &Value) -> ActionResult {
        ActionResult::err("需配置 OPENSUBS_API_KEY 环境变量")
    }

    async fn transcode(&self, args: &Value) -> ActionResult {
        let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
        let output = args.get("output").and_then(|v| v.as_str()).unwrap_or("");
        if input.is_empty() || output.is_empty() { return ActionResult::err("缺少 input/output"); }
        match tokio::process::Command::new("ffmpeg").args(["-i", input, "-c:v", "libx264", "-c:a", "aac", "-y", output]).output().await {
            Ok(o) if o.status.success() => ActionResult::ok(json!({"output": output}), format!("转码完成: {output}")),
            Ok(o) => ActionResult::err(format!("转码失败: {}", String::from_utf8_lossy(&o.stderr).chars().take(300).collect::<String>())),
            Err(_) => ActionResult::err("ffmpeg 不可用"),
        }
    }

    async fn media_library_scan(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/srv/media");
        match tokio::process::Command::new("find")
            .args([path, "-type", "f", "(", "-name", "*.mp4", "-o", "-name", "*.mkv", "-o", "-name", "*.avi", "-o", "-name", "*.mp3", "-o", "-name", "*.flac", ")"])
            .output().await
        {
            Ok(o) if o.status.success() => {
                let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
                let files: Vec<&str> = stdout_str.lines().collect();
                let mut library = Vec::new();
                for f in &files {
                    let filename = std::path::Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    let ext = std::path::Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("");
                    let media_type = match ext { "mp4"|"mkv"|"avi" => "video", "mp3"|"flac" => "audio", _ => "other" };
                    library.push(json!({"path": f, "type": media_type, "filename": filename}));
                }
                ActionResult::ok(json!({"total": files.len(), "library": library}), format!("媒体库: {} 个文件", files.len()))
            }
            _ => ActionResult::err("扫描失败"),
        }
    }

    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let data = json!({"note": "media agent smart mode"});
        let library = match ModelLibrary::load() { Ok(l) => l, Err(e) => return ActionResult::err(format!("模型库: {e}")) };
        match llm_helper::smart_analyze("media", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(json!({"analysis": analysis}), "媒体智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM: {e:#}")),
        }
    }
}

fn urlenc(s: &str) -> String {
    s.chars().map(|c| match c { ' ' => "%20".into(), _ => c.to_string() }).collect()
}
