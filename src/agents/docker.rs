//! docker-agent - 容器管理(预设执行体)。
//!
//! 域:容器启停/日志/镜像管理/健康检查/compose 编排/资源监控/容器内执行。全用 podman CLI。

use serde_json::{json, Value};
use std::time::Duration;

use crate::agents::{ActionResult, llm_helper};
use crate::models::ModelLibrary;

pub struct DockerAgent;

impl DockerAgent {
    pub async fn execute(&self, action: &str, args: &Value) -> ActionResult {
        match action {
            "list_containers" => self.list_containers().await,
            "start_container" => self.container_op("start", args).await,
            "stop_container" => self.container_op("stop", args).await,
            "restart_container" => self.container_op("restart", args).await,
            "container_logs" => self.container_logs(args).await,
            "container_stats" => self.container_stats(args).await,
            "container_exec" => self.container_exec(args).await,
            "prune_images" => self.prune_images().await,
            "pull_image" => self.pull_image(args).await,
            "list_images" => self.list_images().await,
            "health_check" => self.health_check().await,
            "compose_up" => self.compose("up", args).await,
            "compose_down" => self.compose("down", args).await,
            "compose_logs" => self.compose_logs(args).await,
            "generate_compose" => self.generate_compose(args).await,
            "deploy_service" => self.deploy_service(args).await,
            _ => self.smart_execute(action, args).await,
        }
    }

    async fn list_containers(&self) -> ActionResult {
        run_podman(&["ps", "-a", "--format", "json"], "列容器").await
    }

    async fn container_op(&self, op: &str, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return ActionResult::err("缺少容器名(name 参数)");
        }
        run_podman(&[op, name], &format!("{op} 容器 {name}")).await
    }

    async fn container_logs(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let tail = args.get("lines").and_then(|v| v.as_str()).unwrap_or("50");
        if name.is_empty() {
            return ActionResult::err("缺少容器名(name 参数)");
        }
        run_podman(&["logs", "--tail", tail, name], &format!("容器 {name} 日志(最后{tail}行)")).await
    }

    /// 容器资源使用(CPU/内存/网络)
    async fn container_stats(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return ActionResult::err("缺少容器名(name 参数)");
        }
        run_podman(&["stats", "--no-stream", "--format", "json", name], &format!("容器 {name} 资源统计")).await
    }

    /// 在容器内执行命令
    async fn container_exec(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || cmd.is_empty() {
            return ActionResult::err("缺少 name/command 参数");
        }
        // 用 bash -c 执行命令
        run_podman(&["exec", name, "bash", "-c", cmd], &format!("容器 {name} 执行: {cmd}")).await
    }

    async fn prune_images(&self) -> ActionResult {
        run_podman(&["image", "prune", "-f"], "清理无用镜像").await
    }

    /// 拉取镜像
    async fn pull_image(&self, args: &Value) -> ActionResult {
        let image = args.get("image").and_then(|v| v.as_str()).unwrap_or("");
        if image.is_empty() {
            return ActionResult::err("缺少 image 参数");
        }
        run_podman(&["pull", image], &format!("拉取镜像 {image}")).await
    }

    /// 列出本地镜像
    async fn list_images(&self) -> ActionResult {
        run_podman(&["image", "ls", "--format", "json"], "列镜像").await
    }

    async fn health_check(&self) -> ActionResult {
        match tokio::process::Command::new("podman")
            .args(["ps", "--format", "{{.Names}} {{.Status}}"])
            .output().await
        {
            Ok(o) if o.status.success() => {
                let containers = String::from_utf8_lossy(&o.stdout).trim().to_string();
                ActionResult::ok(json!({"containers": containers}), "容器健康检查完成")
            }
            Ok(o) => ActionResult::err(format!("podman 失败: {}", String::from_utf8_lossy(&o.stderr))),
            Err(_) => ActionResult::err("podman 不可用"),
        }
    }

    /// podman compose up/down(回退到 podman run)
    async fn compose(&self, op: &str, args: &Value) -> ActionResult {
        let file = args.get("file").and_then(|v| v.as_str()).unwrap_or("docker-compose.yml");
        if op == "up" {
            // 读 compose 文件,解析出 image/ports/volumes/env,用 podman run 启动
            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(_) => return ActionResult::err(format!("compose 文件不存在: {file}")),
            };
            // 简单 YAML 解析(不引入 yaml crate)
            let mut image = String::new();
            let mut name = String::new();
            let mut ports: Vec<String> = Vec::new();
            let mut volumes: Vec<String> = Vec::new();
            let mut envs: Vec<String> = Vec::new();
            let mut network = String::new();
            let mut in_services = false;
            let mut in_service = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("services:") { in_services = true; continue; }
                if in_services && !line.starts_with(' ') && !trimmed.is_empty() { in_service = false; }
                if in_services && line.starts_with("  ") && !line.starts_with("    ") {
                    in_service = true;
                    name = trimmed.trim_end_matches(':').to_string();
                    continue;
                }
                if in_service {
                    if let Some(v) = trimmed.strip_prefix("image: ") { image = v.trim().to_string(); }
                    else if trimmed.starts_with("- \"") && ports.is_empty() {
                        // could be ports or volumes, check context
                    }
                    if let Some(v) = trimmed.strip_prefix("network_mode: ") { network = v.trim().to_string(); }
                }
            }
            // 重新解析 ports/volumes/env(按段)
            let mut current_section = "";
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "ports:" { current_section = "ports"; continue; }
                if trimmed == "volumes:" { current_section = "volumes"; continue; }
                if trimmed == "environment:" { current_section = "env"; continue; }
                if trimmed.starts_with('-') {
                    let val = trimmed.trim_start_matches("- ").trim_matches('"').to_string();
                    match current_section {
                        "ports" => ports.push(val),
                        "volumes" => volumes.push(val),
                        "env" => envs.push(val),
                        _ => {}
                    }
                }
                if !trimmed.starts_with('-') && !trimmed.ends_with(':') && current_section != "" {
                    if !line.starts_with("    ") { current_section = ""; }
                }
            }
            if image.is_empty() {
                return ActionResult::err("compose 文件中未找到 image");
            }
            // 构建 podman run 命令
            let mut cmd_args = vec!["run", "-d", "--name", &name, "--restart=unless-stopped"];
            for p in &ports { cmd_args.push("-p"); cmd_args.push(p.as_str()); }
            for v in &volumes { cmd_args.push("-v"); cmd_args.push(v.as_str()); }
            for e in &envs { cmd_args.push("-e"); cmd_args.push(e.as_str()); }
            if !network.is_empty() { cmd_args.push("--network"); cmd_args.push(&network); }
            cmd_args.push(&image);
            run_podman(&cmd_args, &format!("启动 {name}({image})")).await
        } else {
            // compose down: podman stop + rm
            let content = std::fs::read_to_string(file).unwrap_or_default();
            let mut name = String::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if line.starts_with("  ") && !line.starts_with("    ") && trimmed.ends_with(':') {
                    name = trimmed.trim_end_matches(':').to_string();
                    break;
                }
            }
            if name.is_empty() {
                return ActionResult::err("compose 文件中未找到服务名");
            }
            let _ = tokio::process::Command::new("podman").args(["stop", &name]).output().await;
            run_podman(&["rm", "-f", &name], &format!("停止并删除 {name}")).await
        }
    }

    /// compose 项目日志
    async fn compose_logs(&self, args: &Value) -> ActionResult {
        let file = args.get("file").and_then(|v| v.as_str()).unwrap_or("docker-compose.yml");
        let tail = args.get("lines").and_then(|v| v.as_str()).unwrap_or("50");
        run_podman(&["compose", "-f", file, "logs", "--tail", tail], &format!("compose 日志({file})")).await
    }

    /// 生成 docker-compose.yml 模板
    async fn generate_compose(&self, args: &Value) -> ActionResult {
        let service_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("app");
        let image = args.get("image").and_then(|v| v.as_str()).unwrap_or("");
        let port = args.get("port").and_then(|v| v.as_str()).unwrap_or("");
        let volume = args.get("volume").and_then(|v| v.as_str()).unwrap_or("");
        let network = args.get("network").and_then(|v| v.as_str()).unwrap_or("host");
        let env_str = args.get("env").and_then(|v| v.as_str()).unwrap_or("");
        let restart = args.get("restart").and_then(|v| v.as_str()).unwrap_or("unless-stopped");

        if image.is_empty() {
            return ActionResult::err("缺少 image 参数(如 docker.io/homeassistant/home-assistant:latest)");
        }

        let mut compose = format!("version: \"3\"\nservices:\n  {service_name}:\n    image: {image}\n    restart: {restart}\n    network_mode: {network}\n");
        if !port.is_empty() {
            compose.push_str(&format!("    ports:\n      - \"{port}\"\n"));
        }
        if !volume.is_empty() {
            compose.push_str(&format!("    volumes:\n      - {volume}\n"));
        }
        if !env_str.is_empty() {
            compose.push_str("    environment:\n");
            for e in env_str.split(',') {
                compose.push_str(&format!("      - {}\n", e.trim()));
            }
        }

        let default_path = format!("/tmp/{}-compose.yml", service_name);
        let output_path = args.get("output").and_then(|v| v.as_str()).unwrap_or(&default_path);
        match std::fs::write(output_path, &compose) {
            Ok(_) => ActionResult::ok(
                json!({"path": output_path, "content": compose}),
                format!("compose 文件已生成: {output_path}")
            ),
            Err(e) => ActionResult::err(format!("写入失败: {e}")),
        }
    }

    /// 部署服务 + 自动纳管:生成 compose -> 启动 -> 等就绪 -> 发现 API -> 生成 KB -> 注册 agent。
    async fn deploy_service(&self, args: &Value) -> ActionResult {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let image = args.get("image").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || image.is_empty() {
            return ActionResult::err("缺少 name/image 参数");
        }
        let port = args.get("port").and_then(|v| v.as_str()).unwrap_or("");
        let base_url = args
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("http://localhost:{}", if port.is_empty() { "8080" } else { port }));
        let volume = args.get("volume").and_then(|v| v.as_str()).unwrap_or("");
        let env_str = args.get("env").and_then(|v| v.as_str()).unwrap_or("");

        // 1. 生成 compose + 启动
        let gen = self
            .generate_compose(&json!({"name": name, "image": image, "port": port, "volume": volume, "network": "host", "env": env_str}))
            .await;
        if !gen.success {
            return gen;
        }
        let file = gen.data.get("path").and_then(|v| v.as_str()).unwrap_or("/tmp/compose.yml");
        let up = self.compose("up", &json!({"file": file})).await;
        if !up.success {
            return ActionResult::err(format!("容器启动失败: {}", up.message));
        }

        // 2. 等服务就绪(任何 HTTP 响应都算就绪)
        if !self.wait_healthy(&base_url).await {
            return ActionResult::ok(
                json!({"service": name, "image": image, "base_url": base_url, "auto_managed": false}),
                format!("部署 {name} 成功,但 {base_url} 未就绪(仅容器层管理,可能需手动配)"),
            );
        }

        // 3. 发现 API + 生成 KB + 注册 agent
        let base_url_env = format!("{}_URL", name.to_uppercase());
        match crate::agents::discover::discover_and_register(name, &base_url, &base_url_env, image, &format!("自动纳管: {image}")).await {
            Ok((count, kb_path)) => ActionResult::ok(
                json!({"service": name, "image": image, "base_url": base_url, "actions": count, "kb": kb_path, "token_env": format!("{}_TOKEN", name.to_uppercase())}),
                format!("部署 {name} 成功,自动发现 {count} 个 action(需配 {}_TOKEN 鉴权)", name.to_uppercase()),
            ),
            Err(e) => ActionResult::ok(
                json!({"service": name, "image": image, "base_url": base_url, "auto_managed": false}),
                format!("部署 {name} 成功,但未发现可用 API(仅容器层管理): {e:#}"),
            ),
        }
    }

    /// 轮询 base_url 直到服务响应(任何 HTTP 状态都算就绪,含 404)。最多 ~30s。
    async fn wait_healthy(&self, base_url: &str) -> bool {
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
            Ok(c) => c,
            Err(_) => return false,
        };
        for _ in 0..15 {
            if client.get(base_url).send().await.map(|r| r.status().as_u16() >= 200).unwrap_or(false) {
                return true;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        false
    }

    /// LLM 智能分析:收集容器数据 -> 域 LLM 诊断 -> 返回智能结果
    async fn smart_execute(&self, action: &str, args: &Value) -> ActionResult {
        let user_request = args.get("input").and_then(|v| v.as_str()).unwrap_or(action);
        let mut data = json!({});

        // 容器列表
        if let Ok(o) = tokio::process::Command::new("podman")
            .args(["ps", "-a", "--format", "json"]).output().await
        {
            if o.status.success() {
                data["containers"] = serde_json::from_str(&String::from_utf8_lossy(&o.stdout)).unwrap_or(Value::Null);
            }
        }

        // 指定容器的日志
        if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
            if !name.is_empty() {
                if let Ok(o) = tokio::process::Command::new("podman")
                    .args(["logs", "--tail", "50", name]).output().await
                {
                    data["logs"] = Value::String(String::from_utf8_lossy(&o.stdout).to_string());
                }
            }
        }

        // 镜像列表
        if let Ok(o) = tokio::process::Command::new("podman")
            .args(["image", "ls", "--format", "json"]).output().await
        {
            if o.status.success() {
                data["images"] = serde_json::from_str(&String::from_utf8_lossy(&o.stdout)).unwrap_or(Value::Null);
            }
        }

        let library = match ModelLibrary::load() {
            Ok(l) => l,
            Err(e) => return ActionResult::err(format!("模型库加载失败: {e}")),
        };

        match llm_helper::smart_analyze("docker", user_request, &data, &library).await {
            Ok(analysis) => ActionResult::ok(
                json!({"analysis": analysis, "raw_data": data}),
                "容器智能诊断完成"
            ),
            Err(e) => ActionResult::err(format!("LLM 分析失败: {e:#}")),
        }
    }
}

async fn run_podman(args: &[&str], label: &str) -> ActionResult {
    match tokio::process::Command::new("podman").args(args).output().await {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if o.status.success() {
                let data: Value = serde_json::from_str(&stdout).unwrap_or(Value::String(stdout.clone()));
                ActionResult::ok(data, format!("{label} 成功"))
            } else {
                ActionResult::err(format!("{label} 失败: {stderr}"))
            }
        }
        Err(_) => ActionResult::err(format!("{label}: podman 不可用")),
    }
}
