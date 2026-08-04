//! filesystem-agent - 文件管理(预设执行体)。
//!
//! 域:共享文件夹/文件列表/搜索/目录大小。用 OMV RPC + CLI。

use serde_json::{json, Value};
use std::collections::HashMap;
use crate::agents::{ActionResult, OMV_NEW_UUID, llm_helper};
use crate::models::ModelLibrary;
use crate::omv::{self, OmvCall};

pub struct FilesystemAgent;

impl FilesystemAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "list_shared_folders" => self.list_shared_folders().await,
            "list_files" => self.list_files(args).await,
            "file_info" => self.file_info(args).await,
            "search_files" => self.search_files(args).await,
            "dir_size" => self.dir_size(args).await,
            "classify_files" => self.classify_files(args).await,
            "dedup" => self.dedup(args).await,
            "tag_file" => self.tag_file(args).await,
            "search_by_tag" => self.search_by_tag(args).await,
            "create_share" => self.create_share(args).await,
            "create_smb_share" => self.create_smb_share(args).await,
            "delete_share" => self.delete_share(args).await,
            "list_tags" => self.list_tags().await,
            "delete_tag" => self.delete_tag(args).await,
            "auto_tag_directory" => self.auto_tag_directory(args).await,
            "move_file" => self.move_file(args).await,
            "copy_file" => self.copy_file(args).await,
            "create_directory" => self.create_directory(args).await,
            "file_permissions" => self.file_permissions(args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    async fn list_shared_folders(&self) -> ActionResult {
        let call = OmvCall::new("ShareMgmt", "enumerateSharedFolders", json!({}), "列共享文件夹");
        match omv::execute(&call).await {
            Ok(r) if r.success => {
                let data: Value = serde_json::from_str(&r.stdout).unwrap_or(Value::String(r.stdout.clone()));
                ActionResult::ok(data, "共享文件夹列表")
            }
            Ok(r) => ActionResult::err(format!("查询失败: {}", r.stderr)),
            Err(e) => ActionResult::err(format!("出错: {e:#}")),
        }
    }

    async fn list_files(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        run_cli("ls", &["-la", "--color=never", path], &format!("列文件 {path}")).await
    }

    async fn file_info(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        run_cli("stat", &[path], &format!("文件信息 {path}")).await
    }

    async fn search_files(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
        run_cli("find", &[path, "-name", pattern, "-maxdepth", "3"], &format!("搜索 {pattern} in {path}")).await
    }

    async fn dir_size(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        run_cli("du", &["-sh", path], &format!("目录大小 {path}")).await
    }

    /// 按扩展名自动分类
    async fn classify_files(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/srv");
        match tokio::process::Command::new("find").args([path, "-type", "f", "-maxdepth", "3"]).output().await {
            Ok(o) if o.status.success() => {
                let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
                let mut categories: HashMap<String, Vec<String>> = HashMap::new();
                for f in stdout_str.lines() {
                    let ext = std::path::Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("other").to_lowercase();
                    let cat = match ext.as_str() {
                        "jpg"|"jpeg"|"png"|"gif"|"heic"|"bmp" => "images",
                        "mp4"|"mkv"|"avi"|"mov"|"flv" => "videos",
                        "mp3"|"flac"|"wav"|"aac" => "audio",
                        "pdf"|"doc"|"docx"|"txt"|"xlsx" => "documents",
                        "zip"|"tar"|"gz"|"rar"|"7z" => "archives",
                        _ => "other",
                    }.to_string();
                    categories.entry(cat).or_default().push(f.to_string());
                }
                ActionResult::ok(json!(categories), format!("分类 {} 文件为 {} 类", stdout_str.lines().count(), categories.len()))
            }
            _ => ActionResult::err("分类失败"),
        }
    }

    /// 按内容去重(md5)
    async fn dedup(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/srv");
        let cmd = format!("find {} -type f -exec md5sum {{}} + 2>/dev/null | sort | awk '{{print $1}}' | uniq -d", path);
        match tokio::process::Command::new("bash").args(["-c", &cmd]).output().await {
            Ok(o) if o.status.success() => {
                let dups = String::from_utf8_lossy(&o.stdout).to_string();
                let count = dups.lines().filter(|l| !l.is_empty()).count();
                ActionResult::ok(json!({"duplicates": dups, "count": count}), format!("{} 个重复", count))
            }
            _ => ActionResult::err("去重检查失败"),
        }
    }

    /// 给文件打标签(JSON 标签库)
    async fn tag_file(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let tag = args.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() || tag.is_empty() { return ActionResult::err("缺少 path/tag 参数"); }
        let db = "/var/lib/aaos/file-tags.json";
        let mut tags: Vec<Value> = std::fs::read_to_string(db).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        tags.push(json!({"path": path, "tag": tag}));
        let _ = std::fs::create_dir_all("/var/lib/aaos");
        let _ = std::fs::write(db, serde_json::to_string_pretty(&tags).unwrap_or_default());
        ActionResult::ok(json!({"path": path, "tag": tag}), format!("已打标签: {tag}"))
    }

    /// 按标签搜文件
    async fn search_by_tag(&self, args: &Value) -> ActionResult {
        let tag = args.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let db = "/var/lib/aaos/file-tags.json";
        let tags: Vec<Value> = std::fs::read_to_string(db).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let matches: Vec<&Value> = tags.iter().filter(|t| t.to_string().to_lowercase().contains(&tag)).collect();
        ActionResult::ok(json!({"tag": tag, "matches": matches.len(), "results": matches}),
            format!("标签 \"{tag}\": {} 匹配", matches.len()))
    }

    /// 创建共享文件夹(查挂载点 + 哨兵 UUID + ShareMgmt::set)
    async fn create_share(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() { return ActionResult::err("缺少 name 参数"); }

        // 1. 查已挂载文件系统 -> 取 devicefile
        let fs_call = OmvCall::new("FileSystemMgmt", "enumerateMountedFilesystems", json!({}), "查挂载文件系统");
        let fs_result = match omv::execute(&fs_call).await {
            Ok(r) if r.success => r,
            Ok(r) => return ActionResult::err(format!("查挂载文件系统失败: {}", r.stderr)),
            Err(e) => return ActionResult::err(format!("查挂载文件系统出错: {e:#}")),
        };
        let fs_list: Vec<Value> = serde_json::from_str(&fs_result.stdout).unwrap_or_default();
        let devicefile = fs_list.first()
            .and_then(|f| f.get("devicefile"))
            .and_then(|d| d.as_str())
            .unwrap_or("");
        if devicefile.is_empty() {
            return ActionResult::err("无可用挂载文件系统,请先格式化并挂载磁盘");
        }

        // 2. 查 FsTab 条目 UUID(OMV 的 mntentref 用这个,不是文件系统 UUID)
        let fstab_call = OmvCall::new("FsTab", "getByFsName", json!({"fsname": devicefile}), "查 FsTab UUID");
        let fstab_result = match omv::execute(&fstab_call).await {
            Ok(r) if r.success => r,
            Ok(r) => return ActionResult::err(format!("查 FsTab 失败: {}", r.stderr.chars().take(200).collect::<String>())),
            Err(e) => return ActionResult::err(format!("查 FsTab 出错: {e:#}")),
        };
        let fstab: Value = serde_json::from_str(&fstab_result.stdout).unwrap_or(json!({}));
        let mntentref = fstab.get("uuid").and_then(|u| u.as_str()).unwrap_or("");
        if mntentref.is_empty() {
            return ActionResult::err(format!("FsTab 中无 {devicefile} 的挂载条目,请先在 OMV 挂载该文件系统"));
        }

        // 3. 用哨兵 UUID 创建共享(reldirpath 用 /name 避免冲突)
        let reldirpath = format!("/{name}");
        let call = OmvCall::new("ShareMgmt", "set", json!({
            "uuid": OMV_NEW_UUID,
            "name": name,
            "reldirpath": reldirpath,
            "comment": name,
            "mntentref": mntentref
        }), &format!("创建共享 {name}"));
        match omv::execute(&call).await {
            Ok(r) if r.success => ActionResult::ok(
                serde_json::from_str(&r.stdout).unwrap_or(Value::String(r.stdout.clone())),
                format!("共享 {name} 创建成功(mntentref={})", &mntentref[..8])),
            Ok(r) => ActionResult::err(format!("创建失败: {}", r.stderr.chars().take(200).collect::<String>())),
            Err(e) => ActionResult::err(format!("创建出错: {e:#}")),
        }
    }

    /// 创建 SMB 共享(查共享 UUID + 哨兵 UUID + SMB::setShare)
    async fn create_smb_share(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() { return ActionResult::err("缺少 name 参数"); }

        // 1. 查共享文件夹 UUID(按名字找)
        let sf_call = OmvCall::new("ShareMgmt", "enumerateSharedFolders", json!({}), "查共享");
        let sf_result = match omv::execute(&sf_call).await {
            Ok(r) if r.success => r,
            _ => return ActionResult::err("查共享文件夹失败"),
        };
        let sf_list: Vec<Value> = serde_json::from_str(&sf_result.stdout).unwrap_or_default();
        let sharedfolderref = sf_list.iter()
            .find(|f| f.get("name").and_then(|n| n.as_str()) == Some(name))
            .and_then(|f| f.get("uuid"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        if sharedfolderref.is_empty() {
            return ActionResult::err(format!("找不到共享文件夹 {name},请先创建"));
        }

        // 2. 用哨兵 UUID 创建 SMB 共享
        let call = OmvCall::new("SMB", "setShare", json!({
            "uuid": OMV_NEW_UUID,
            "enable": true,
            "sharedfolderref": sharedfolderref,
            "comment": name,
            "guest": "no",
            "readonly": false,
            "browseable": true,
            "recyclebin": false,
            "recyclemaxsize": 0,
            "recyclemaxage": 0,
            "hidedotfiles": false,
            "inheritacls": false,
            "inheritpermissions": false,
            "easupport": false,
            "storedosattributes": false,
            "hostsallow": "",
            "hostsdeny": "",
            "audit": false,
            "extraoptions": ""
        }), &format!("创建 SMB 共享 {name}"));
        match omv::execute(&call).await {
            Ok(r) if r.success => ActionResult::ok(serde_json::from_str(&r.stdout).unwrap_or(serde_json::Value::String(r.stdout.clone())), format!("SMB 共享 {name} 创建成功")),
            Ok(r) => ActionResult::err(format!("创建失败: {}", r.stderr.chars().take(200).collect::<String>())),
            Err(e) => ActionResult::err(format!("创建出错: {e:#}")),
        }
    }

    /// 删除共享文件夹(破坏性)
    async fn delete_share(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() { return ActionResult::err("缺少 name 参数"); }

        // 查 UUID
        let sf_call = OmvCall::new("ShareMgmt", "enumerateSharedFolders", json!({}), "查共享");
        let sf_result = match omv::execute(&sf_call).await {
            Ok(r) if r.success => r,
            _ => return ActionResult::err("查共享失败"),
        };
        let sf_list: Vec<Value> = serde_json::from_str(&sf_result.stdout).unwrap_or_default();
        let uuid = sf_list.iter()
            .find(|f| f.get("name").and_then(|n| n.as_str()) == Some(name))
            .and_then(|f| f.get("uuid"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        if uuid.is_empty() {
            return ActionResult::err(format!("找不到共享 {name}"));
        }

        let call = OmvCall::new("ShareMgmt", "delete", json!({"uuid": uuid, "recursive": false}), &format!("删除共享 {name}"));
        match omv::execute(&call).await {
            Ok(r) if r.success => ActionResult::ok(serde_json::from_str(&r.stdout).unwrap_or(serde_json::Value::String(r.stdout.clone())), format!("共享 {name} 已删除")),
            Ok(r) => ActionResult::err(format!("删除失败: {}", r.stderr.chars().take(200).collect::<String>())),
            Err(e) => ActionResult::err(format!("删除出错: {e:#}")),
        }
    }

    // ===== 标签系统增强 =====

    /// 列出所有标签
    async fn list_tags(&self) -> ActionResult {
        let db = "/var/lib/aaos/file-tags.json";
        let tags: Vec<Value> = std::fs::read_to_string(db).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // 按标签分组统计
        let mut tag_counts: HashMap<String, Vec<String>> = HashMap::new();
        for t in &tags {
            let tag = t.get("tag").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let path = t.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string();
            tag_counts.entry(tag).or_default().push(path);
        }
        let summary: Vec<Value> = tag_counts.iter().map(|(tag, paths)| {
            json!({"tag": tag, "count": paths.len(), "files": paths})
        }).collect();
        ActionResult::ok(json!({"total_tags": summary.len(), "total_files": tags.len(), "tags": summary}),
            format!("共 {} 个标签,标记 {} 个文件", summary.len(), tags.len()))
    }

    /// 删除标签
    async fn delete_tag(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let tag = args.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() || tag.is_empty() { return ActionResult::err("缺少 path/tag 参数"); }
        let db = "/var/lib/aaos/file-tags.json";
        let mut tags: Vec<Value> = std::fs::read_to_string(db).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let before = tags.len();
        tags.retain(|t| {
            !(t.get("path").and_then(|p| p.as_str()) == Some(path) && t.get("tag").and_then(|x| x.as_str()) == Some(tag))
        });
        let removed = before - tags.len();
        let _ = std::fs::write(db, serde_json::to_string_pretty(&tags).unwrap_or_default());
        ActionResult::ok(json!({"removed": removed}), format!("删除 {removed} 个标签"))
    }

    /// 自动标签目录(按文件类型+扩展名批量打标)
    async fn auto_tag_directory(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() { return ActionResult::err("缺少 path 参数"); }
        // 扫描文件
        match tokio::process::Command::new("find").args([path, "-type", "f", "-maxdepth", "3"]).output().await {
            Ok(o) if o.status.success() => {
                let stdout_str = String::from_utf8_lossy(&o.stdout).to_string();
                let db = "/var/lib/aaos/file-tags.json";
                let mut tags: Vec<Value> = std::fs::read_to_string(db).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                let mut tagged = 0;
                for f in stdout_str.lines() {
                    let ext = std::path::Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    let tag = match ext.as_str() {
                        "jpg"|"jpeg"|"png"|"gif"|"heic"|"bmp" => "photo",
                        "mp4"|"mkv"|"avi"|"mov"|"flv" => "video",
                        "mp3"|"flac"|"wav"|"aac" => "audio",
                        "pdf"|"doc"|"docx"|"txt"|"xlsx" => "document",
                        "zip"|"tar"|"gz"|"rar"|"7z" => "archive",
                        "iso"|"img" => "disk-image",
                        "sh"|"py"|"rs"|"js"|"ts" => "code",
                        _ => "other",
                    };
                    // 避免重复打标
                    let already = tags.iter().any(|t| t.get("path").and_then(|p| p.as_str()) == Some(f) && t.get("tag").and_then(|x| x.as_str()) == Some(tag));
                    if !already {
                        tags.push(json!({"path": f, "tag": tag}));
                        tagged += 1;
                    }
                }
                let _ = std::fs::create_dir_all("/var/lib/aaos");
                let _ = std::fs::write(db, serde_json::to_string_pretty(&tags).unwrap_or_default());
                ActionResult::ok(json!({"tagged": tagged, "total_scanned": stdout_str.lines().count()}),
                    format!("自动标签: {tagged} 个文件打标({} 个已扫描)", stdout_str.lines().count()))
            }
            _ => ActionResult::err("扫描失败"),
        }
    }

    // ===== 文件操作 =====

    /// 移动文件
    async fn move_file(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest 参数"); }
        run_cli("mv", &[src, dest], &format!("移动 {src} -> {dest}")).await
    }

    /// 复制文件
    async fn copy_file(&self, args: &Value) -> ActionResult {
        let src = args.get("src").and_then(|v| v.as_str()).unwrap_or("");
        let dest = args.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        if src.is_empty() || dest.is_empty() { return ActionResult::err("缺少 src/dest 参数"); }
        run_cli("cp", &["-r", src, dest], &format!("复制 {src} -> {dest}")).await
    }

    /// 创建目录
    async fn create_directory(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() { return ActionResult::err("缺少 path 参数"); }
        run_cli("mkdir", &["-p", path], &format!("创建目录 {path}")).await
    }

    /// 查看文件权限
    async fn file_permissions(&self, args: &Value) -> ActionResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() { return ActionResult::err("缺少 path 参数"); }
        run_cli("ls", &["-la", path], &format!("权限 {path}")).await
    }

    /// LLM 智能分析:收集文件数据 -> 域 LLM 分析
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/srv");
        let mut data = json!({});

        // 共享文件夹
        let sf_call = OmvCall::new("ShareMgmt", "enumerateSharedFolders", json!({}), "查共享");
        if let Ok(r) = omv::execute(&sf_call).await {
            if r.success { data["shared_folders"] = serde_json::from_str(&r.stdout).unwrap_or(Value::Null); }
        }
        // 目录文件
        if let Ok(o) = tokio::process::Command::new("ls").args(["-la", path]).output().await {
            data["files"] = Value::String(String::from_utf8_lossy(&o.stdout).to_string());
        }
        // 目录大小
        if let Ok(o) = tokio::process::Command::new("du").args(["-sh", path]).output().await {
            data["dir_size"] = Value::String(String::from_utf8_lossy(&o.stdout).trim().to_string());
        }
        // 标签库
        if let Ok(tags) = std::fs::read_to_string("/var/lib/aaos/file-tags.json") {
            data["tags"] = serde_json::from_str(&tags).unwrap_or(Value::Null);
        }

        let library = match ModelLibrary::load() {
            Ok(l) => l,
            Err(e) => return ActionResult::err(format!("模型库加载失败: {e}")),
        };
        match llm_helper::smart_analyze("filesystem", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(json!({"analysis": analysis, "raw_data": data}), "文件系统智能分析完成"),
            Err(e) => ActionResult::err(format!("LLM 分析失败: {e:#}")),
        }
    }
}

async fn run_cli(cmd: &str, args: &[&str], label: &str) -> ActionResult {
    match tokio::process::Command::new(cmd).args(args).output().await {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            ActionResult::ok(Value::String(out), format!("{label} 成功"))
        }
        Ok(o) => ActionResult::err(format!("{label} 失败: {}", String::from_utf8_lossy(&o.stderr))),
        Err(_) => ActionResult::err(format!("{label}: {cmd} 不可用")),
    }
}
