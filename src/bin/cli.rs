use aaos::agents::{self, Intent};
use aaos::ipc::{connect, write_message, Message};
use aaos::llm;
use aaos::models::ModelLibrary;
use serde_json::json;
use anyhow::Result;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "aaos-cli")]
#[command(about = "AAOS CLI - interact with Core Agent / model library")]
struct Args {
    /// Path to Core Agent's IPC socket
    #[arg(long, default_value = "/run/aaos/core.sock")]
    socket: String,

    /// 自然语言输入
    input: Vec<String>,

    /// 确认破坏性命令(带 token)
    #[arg(long)]
    confirm: Option<String>,

    /// 列出模型库里的 provider + 模型
    #[arg(long)]
    list_models: bool,

    /// 从指定 provider 自动发现模型(调 /v1/models)
    #[arg(long, value_name = "PROVIDER_ID")]
    discover_models: Option<String>,

    /// 测试 Core LLM 连通性(发一条 trivial prompt,排查 base_url/key/model)
    #[arg(long)]
    test_llm: bool,

    /// 每日巡检:system-agent 全面检查 + Core 翻 NL 报告
    #[arg(long)]
    daily_check: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let args = Args::parse();

    // ---- 模型库子功能(不走 Core socket)----
    if args.list_models {
        return list_models();
    }
    if let Some(provider_id) = args.discover_models {
        return discover_models(&provider_id).await;
    }
    if args.test_llm {
        return test_llm().await;
    }
    if args.daily_check {
        return daily_check().await;
    }

    // ---- 默认:自然语言聊天 ----
    let input = args.input.join(" ");
    if input.is_empty() && args.confirm.is_none() {
        anyhow::bail!("必须提供自然语言输入,或 --confirm <token>,或 --list-models/--discover-models");
    }

    info!("aaos-cli connecting to {}", args.socket);
    let stream = connect(&args.socket).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    let req = Message::Request {
        id: uuid().into(),
        input,
        confirmation_token: args.confirm,
    };
    write_message(&mut write, &req).await?;

    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        anyhow::bail!("连接关闭前未收到响应");
    }
    let resp = Message::decode(&line)?;
    match resp {
        Message::Response { status, output, confirmation_token, .. } => match status {
            aaos::ipc::ResponseStatus::Success => println!("{}", output),
            aaos::ipc::ResponseStatus::NeedsConfirmation => {
                println!("{}", output);
                if let Some(token) = confirmation_token {
                    println!("\n确认命令: aaos-cli --confirm {token}");
                }
            }
            aaos::ipc::ResponseStatus::Error => {
                eprintln!("[错误] {}", output);
                std::process::exit(1);
            }
        },
        other => anyhow::bail!("意外响应: {other:?}"),
    }

    Ok(())
}

fn list_models() -> Result<()> {
    let lib = ModelLibrary::load()?;
    println!("当前默认 provider: {}", lib.current);
    println!("\n[Core] {} / {}", lib.core.provider, lib.core.model);
    if !lib.core.fallback_provider.is_empty() {
        println!("  fallback: {} / {}", lib.core.fallback_provider, lib.core.fallback_model);
    }
    for ea in &lib.execution_agents {
        println!("[ExecAgent:{}] {} / {}", ea.agent_type, ea.provider, ea.model);
    }
    if !lib.sentinel.provider.is_empty() {
        println!("[Sentinel] {} / {}", lib.sentinel.provider, lib.sentinel.model);
    } else {
        println!("[Sentinel] (纯规则,无 LLM)");
    }
    println!("\nProviders:");
    for p in &lib.providers {
        let failover = if p.in_failover_queue { " [failover]" } else { "" };
        println!("  {} ({}) {} {}{} - {} models",
            p.id, p.category, p.provider_type, p.base_url, failover, p.models.len());
        for m in &p.models {
            println!("    - {} caps={:?} tags={:?}", m.id, m.capabilities, m.tags);
        }
    }
    Ok(())
}

async fn discover_models(provider_id: &str) -> Result<()> {
    let lib = ModelLibrary::load()?;
    let provider = lib.find_provider(provider_id)
        .ok_or_else(|| anyhow::anyhow!("provider '{}' not in models.toml", provider_id))?;
    println!("发现 {} ({}) 的模型...", provider.id, provider.base_url);
    let models = provider.discover_models().await?;
    if models.is_empty() {
        println!("  (未发现任何模型,检查 base_url / api_key)");
        return Ok(());
    }
    println!("发现 {} 个模型:", models.len());
    for m in models {
        println!("  - {} caps={:?} tags={:?}", m.id, m.capabilities, m.tags);
    }
    println!("\n(可把需要的 [[providers.models]] 条目抄进 models.toml,tags 按需调整)");
    Ok(())
}

async fn test_llm() -> Result<()> {
    let lib = ModelLibrary::load()?;
    println!("测试 Core LLM 连通性...");
    match llm::test_connection(&lib).await {
        Ok(resp) => {
            println!("✓ 连通成功,模型回复:");
            println!("  {}", resp.chars().take(500).collect::<String>());
        }
        Err(e) => {
            println!("✗ 连通失败:");
            println!("  {e:#}");
        }
    }
    Ok(())
}

async fn daily_check() -> Result<()> {
    let lib = ModelLibrary::load()?;
    println!("执行每日系统巡检...");
    let intent = Intent {
        agent: "system".into(),
        action: "check_state".into(),
        args: json!({"area": "all"}),
    };
    let result = agents::dispatch(&intent).await;
    if result.success {
        let nl = llm::result_to_nl("每日系统巡检", &result, &lib)
            .await
            .unwrap_or_else(|_| format!("{}\n{}", result.message, result.data));
        println!("{}", nl);
    } else {
        println!("巡检失败: {}", result.message);
    }
    Ok(())
}

fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", nanos)
}
