use aaos::agents::{self, dispatch_plan};
use aaos::intent;
use aaos::ipc::{
    connect, listen, write_message, EventSource, Message, ResponseStatus, Severity,
};
use aaos::llm;
use aaos::models::ModelLibrary;
use aaos::omv::{self, OmvCall};
use anyhow::Result;
use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "aaos-core")]
#[command(about = "AAOS Core Agent - AI runtime for OpenMediaVault")]
struct Args {
    #[arg(long, default_value = "/run/aaos/core.sock")]
    socket: String,
    #[arg(long, default_value = "/run/aaos/sentinel.sock")]
    sentinel_socket: String,
}

#[derive(Debug, Clone)]
struct PendingDestructive {
    call: OmvCall,
    request_id: String,
}

/// 待确认的破坏性 Intent/Plan
#[derive(Debug, Clone)]
struct PendingIntent {
    intent: Option<aaos::agents::Intent>,
    plan: Option<aaos::agents::Plan>,
    request_id: String,
}

/// 判断 action 名是否破坏性
fn is_destructive_action(action: &str) -> bool {
    let a = action.to_lowercase();
    a.contains("delete") || a.contains("destroy") || a.contains("remove") || a.contains("wipe") || a.contains("prune")
}

/// 判断 Plan 是否含破坏性步骤
fn plan_has_destructive(plan: &aaos::agents::Plan) -> bool {
    plan.steps.iter().any(|s| is_destructive_action(&s.action))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    info!(
        "aaos-core starting, socket: {}, sentinel: {}",
        args.socket, args.sentinel_socket
    );

    let listener = listen(&args.socket).await?;
    let pending: Arc<Mutex<HashMap<String, PendingDestructive>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pending_intents: Arc<Mutex<HashMap<String, PendingIntent>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let library = Arc::new(ModelLibrary::load().unwrap_or_else(|e| {
        warn!("加载模型库失败,LLM 禁用,回退规则: {e:#}");
        ModelLibrary {
            current: String::new(),
            providers: vec![],
            core: Default::default(),
            execution_agents: vec![],
            sentinel: Default::default(),
        }
    }));
    if !library.core.provider.is_empty() {
        info!("Core LLM 启用: {} / {}", library.core.provider, library.core.model);
    } else {
        info!("Core LLM 未配置,纯规则模式");
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => warn!("aaos-core received SIGINT, shutting down"),
        res = run(listener, &args.sentinel_socket, pending, pending_intents, library) => {
            if let Err(e) = res { tracing::error!("core run error: {e:#}"); }
        }
    }
    Ok(())
}

async fn run(
    listener: tokio::net::UnixListener,
    sentinel_socket: &str,
    pending: Arc<Mutex<HashMap<String, PendingDestructive>>>,
    pending_intents: Arc<Mutex<HashMap<String, PendingIntent>>>,
    library: Arc<ModelLibrary>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let pending = pending.clone();
        let pending_intents = pending_intents.clone();
        let library = library.clone();
        let sentinel_socket = sentinel_socket.to_string();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &sentinel_socket, pending, pending_intents, library).await {
                tracing::warn!("connection error: {e:#}");
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    sentinel_socket: &str,
    pending: Arc<Mutex<HashMap<String, PendingDestructive>>>,
    pending_intents: Arc<Mutex<HashMap<String, PendingIntent>>>,
    library: Arc<ModelLibrary>,
) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }
        let msg = match Message::decode(&line) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("invalid message: {e:#}");
                continue;
            }
        };
        match msg {
            Message::Request { id, input, confirmation_token } => {
                handle_request(id, input, confirmation_token, &mut write, sentinel_socket, pending.clone(), pending_intents.clone(), library.clone()).await;
            }
            other => tracing::warn!("unexpected message: {other:?}"),
        }
    }
}

async fn handle_request(
    id: String,
    input: String,
    confirmation_token: Option<String>,
    write: &mut tokio::net::unix::OwnedWriteHalf,
    sentinel_socket: &str,
    pending: Arc<Mutex<HashMap<String, PendingDestructive>>>,
    pending_intents: Arc<Mutex<HashMap<String, PendingIntent>>>,
    library: Arc<ModelLibrary>,
) {
    // 确认破坏性调用(旧 OmvCall 确认 + 新 Intent/Plan 确认)
    if let Some(token) = confirmation_token {
        // 先查旧 OmvCall pending
        let call = { pending.lock().await.remove(&token) };
        if let Some(p) = call {
            let result = omv::execute(&p.call).await;
            let resp = match result { Ok(r) => format_result(&r), Err(e) => format!("执行失败: {e:#}") };
            let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: resp, confirmation_token: None }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_executed",
                serde_json::json!({"call": p.call.display(), "description": p.call.description})).await;
            return;
        }
        // 再查 Intent/Plan pending
        let pending_item = { pending_intents.lock().await.remove(&token) };
        match pending_item {
            Some(pi) => {
                // 执行确认后的 Intent 或 Plan
                if let Some(plan) = &pi.plan {
                    info!("确认执行 Plan: {} 步", plan.steps.len());
                    let results = dispatch_plan(plan).await;
                    let combined = serde_json::json!({
                        "steps": results.iter().enumerate().map(|(i, r)| serde_json::json!({
                            "step": i + 1, "success": r.success, "message": r.message, "data": r.data
                        })).collect::<Vec<_>>(),
                        "all_success": results.iter().all(|r| r.success),
                    });
                    let combined_result = agents::ActionResult {
                        success: results.iter().all(|r| r.success),
                        data: combined,
                        message: format!("{} 步执行完成", results.len()),
                    };
                    let nl = llm::result_to_nl(&input, &combined_result, &library).await
                        .unwrap_or_else(|_| format!("{}\n{}", combined_result.message, combined_result.data));
                    let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: nl, confirmation_token: None }).await;
                } else if let Some(intent) = &pi.intent {
                    info!("确认执行 Intent: agent={} action={}", intent.agent, intent.action);
                    let result = agents::dispatch(intent).await;
                    let nl = llm::result_to_nl(&input, &result, &library).await
                        .unwrap_or_else(|_| format!("{}\n{}", result.message, result.data));
                    let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: nl, confirmation_token: None }).await;
                }
                send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_confirmed",
                    serde_json::json!({})).await;
                return;
            }
            None => {
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Error, output: "无效或过期的确认 token".into(), confirmation_token: None }).await;
                return;
            }
        }
    }

    // 新请求
    send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.request",
        serde_json::json!({"input": input})).await;

    // === 快速回复:规则匹配的常见查询直接走 Intent,跳 LLM(秒回)===
    if let Some(action) = intent::parse(&input) {
        if let intent::Action::Call(call) = &action {
            // 只读 + 非破坏性 -> 直接执行,不调 LLM
            if !call.needs_confirmation() {
                info!("快速回复(规则匹配,跳 LLM): {}", call.display());
                let result = omv::execute(call).await;
                let resp = match &result { Ok(r) => format_result(r), Err(e) => format!("执行失败: {e:#}") };
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: resp, confirmation_token: None }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.quick_reply",
                    serde_json::json!({"call": call.display()})).await;
                return;
            }
            // 破坏性 -> 走确认流程
            let token = uuid();
            { pending.lock().await.insert(token.clone(), PendingDestructive { call: call.clone(), request_id: id.clone() }); }
            let _ = write_message(write, &Message::Response { id, status: ResponseStatus::NeedsConfirmation,
                output: format!("⚠️ 破坏性操作: {}\n请用 --confirm {} 确认。", call.display(), token),
                confirmation_token: Some(token.clone()) }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_pending",
                serde_json::json!({"call": call.display(), "confirmation_token": token})).await;
            return;
        }
        if let intent::Action::ApplyChanges = &action {
            info!("快速回复: 应用配置(规则匹配)");
            let result = omv::apply_changes().await;
            let resp = match result { Ok(r) => format_result(&r), Err(e) => format!("应用失败: {e:#}") };
            let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: resp, confirmation_token: None }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.applied", serde_json::json!({"quick": true})).await;
            return;
        }
    }

    // === LLM 调度(规则没匹配的复杂查询)===
    let scheduled = llm::schedule_to_intent(&input, &library).await;
    if let Ok(Some(raw)) = scheduled {
        // 先试 Plan(多步)
        if let Ok(Some(plan)) = llm::parse_plan(&raw) {
            info!("Core 多步计划: {} 步", plan.steps.len());
            // 破坏性检查:Plan 含破坏性步骤 -> 需确认
            if plan_has_destructive(&plan) {
                let destructive_steps: Vec<String> = plan.steps.iter()
                    .filter(|s| is_destructive_action(&s.action))
                    .map(|s| format!("{}::{}", s.agent, s.action))
                    .collect();
                let token = uuid();
                { pending_intents.lock().await.insert(token.clone(), PendingIntent {
                    intent: None, plan: Some(plan), request_id: id.clone(),
                }); }
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::NeedsConfirmation,
                    output: format!("⚠️ 此操作含破坏性步骤: {}\n请用 --confirm {} 确认执行。", destructive_steps.join(", "), token),
                    confirmation_token: Some(token.clone()) }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_pending_plan",
                    serde_json::json!({"steps": destructive_steps, "confirmation_token": token})).await;
                return;
            }
            send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.plan_scheduled",
                serde_json::json!({"steps": plan.steps.len()})).await;
            let results = dispatch_plan(&plan).await;
            // 合并多步结果
            let combined = serde_json::json!({
                "steps": results.iter().enumerate().map(|(i, r)| serde_json::json!({
                    "step": i + 1,
                    "success": r.success,
                    "message": r.message,
                    "data": r.data
                })).collect::<Vec<_>>(),
                "all_success": results.iter().all(|r| r.success),
            });
            let combined_result = agents::ActionResult {
                success: results.iter().all(|r| r.success),
                data: combined,
                message: format!("{} 步执行完成", results.len()),
            };
            let nl = llm::result_to_nl(&input, &combined_result, &library).await
                .unwrap_or_else(|_| format!("{}\n{}", combined_result.message, combined_result.data));
            let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: nl, confirmation_token: None }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.plan_result",
                serde_json::json!({"steps": results.len(), "all_success": combined_result.success})).await;
            return;
        }
        // 再试单步 Intent
        match llm::parse_intent(&raw) {
            Ok(intent) => {
                // 破坏性检查:action 含 delete/destroy/remove/wipe/prune -> 需确认
                if is_destructive_action(&intent.action) {
                    let token = uuid();
                    { pending_intents.lock().await.insert(token.clone(), PendingIntent {
                        intent: Some(intent.clone()), plan: None, request_id: id.clone(),
                    }); }
                    let _ = write_message(write, &Message::Response { id, status: ResponseStatus::NeedsConfirmation,
                        output: format!("⚠️ 破坏性操作: {}::{}\n请用 --confirm {} 确认执行。", intent.agent, intent.action, token),
                        confirmation_token: Some(token.clone()) }).await;
                    send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_pending",
                        serde_json::json!({"agent": intent.agent, "action": intent.action, "confirmation_token": token})).await;
                    return;
                }
                // 正常调度:Intent -> agent 执行 -> result -> NL
                info!("Core 调度: agent={} action={}", intent.agent, intent.action);
                send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.scheduled",
                    serde_json::json!({"agent": intent.agent, "action": intent.action})).await;
                let result = agents::dispatch(&intent).await;
                // 无论成功失败,都翻译结果给用户(agent 报错也翻 NL,不回退规则)
                let nl = llm::result_to_nl(&input, &result, &library).await
                    .unwrap_or_else(|_| format!("{}\n{}", result.message, result.data));
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: nl, confirmation_token: None }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.agent_result",
                    serde_json::json!({"agent": intent.agent, "action": intent.action, "success": result.success})).await;
                return;
            }
            Err(_) => {
                // LLM 直接回复(已通过 execute_action 工具执行了操作,或预检不过)
                info!("Core 直接回复(工具执行完毕或预检不过)");
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: raw, confirmation_token: None }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.direct_reply",
                    serde_json::json!({})).await;
                return;
            }
        }
    }

    // === 回退:规则意图 -> 直接 OMV ===
    let action = match intent::parse(&input) {
        Some(a) => a,
        None => {
            let _ = write_message(write, &Message::Response { id: id.clone(), status: ResponseStatus::Error, output: format!("无法理解意图: {input}"), confirmation_token: None }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.intent_unknown",
                serde_json::json!({"input": input})).await;
            return;
        }
    };

    match action {
        intent::Action::ApplyChanges => {
            send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.apply_requested", serde_json::json!({})).await;
            let result = omv::apply_changes().await;
            let success = result.is_ok();
            let resp = match result { Ok(r) => format_result(&r), Err(e) => format!("应用失败: {e:#}") };
            let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: resp, confirmation_token: None }).await;
            send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.applied", serde_json::json!({"success": success})).await;
        }
        intent::Action::Call(call) => {
            info!("规则意图: {} (class={:?})", call.display(), call.class);
            if call.needs_confirmation() {
                let token = uuid();
                { pending.lock().await.insert(token.clone(), PendingDestructive { call: call.clone(), request_id: id.clone() }); }
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::NeedsConfirmation,
                    output: format!("⚠️  破坏性操作,需确认。\n  调用: {}\n  描述: {}\n  分类: {:?}\n  请用 --confirm {} 确认。", call.display(), call.description, call.class, token),
                    confirmation_token: Some(token.clone()) }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Warning, "ai.destructive_pending",
                    serde_json::json!({"call": call.display(), "confirmation_token": token})).await;
            } else {
                let result = omv::execute(&call).await;
                let resp = match result { Ok(r) => format_result(&r), Err(e) => format!("执行失败: {e:#}") };
                let _ = write_message(write, &Message::Response { id, status: ResponseStatus::Success, output: resp, confirmation_token: None }).await;
                send_event(sentinel_socket, EventSource::Ai, Severity::Info, "ai.executed",
                    serde_json::json!({"call": call.display(), "class": format!("{:?}", call.class)})).await;
            }
        }
    }
}

fn format_result(r: &omv::ExecutionResult) -> String {
    let mut out = String::new();
    if !r.stdout.is_empty() { out.push_str(&r.stdout); }
    if !r.stderr.is_empty() { if !out.is_empty() { out.push('\n'); } out.push_str("[stderr] "); out.push_str(&r.stderr); }
    if !r.success { out.push_str(&format!("\n[exit: {:?}]", r.exit_code)); }
    if r.async_waited { out.push_str("\n(async background task)"); }
    if out.is_empty() { out.push_str("(无输出)"); }
    out
}

async fn send_event(sentinel_socket: &str, source: EventSource, severity: Severity, event_type: &str, payload: serde_json::Value) {
    let msg = Message::Event { timestamp: chrono::Utc::now().to_rfc3339(), source, severity, event_type: event_type.into(), payload };
    match connect(sentinel_socket).await {
        Ok(stream) => {
            let (_, mut w) = stream.into_split();
            if let Err(e) = write_message(&mut w, &msg).await { tracing::warn!("send event to sentinel failed: {e:#}"); }
        }
        Err(e) => tracing::warn!("connect to sentinel failed: {e:#}"),
    }
}

fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", nanos)
}
