use aaos::ipc::{listen, write_message, Message, EventSource, Severity};
use anyhow::Result;
use clap::Parser;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{info, warn};

/// AAOS Sentinel - 全局监管与审计日志。
///
/// 监听 IPC socket,接收 Core 发来的事件(ai.request / ai.executed / ai.destructive_*
/// / ai.applied 等),写入审计日志。MVP 阶段只做被动日志,主动监控后续走 OMV RPC。
#[derive(Parser, Debug)]
#[command(name = "aaos-sentinel")]
#[command(about = "AAOS Sentinel - global oversight and logging")]
struct Args {
    /// IPC socket 路径
    #[arg(long, default_value = "/run/aaos/sentinel.sock")]
    socket: String,

    /// 审计日志文件
    #[arg(long, default_value = "/var/log/aaos/sentinel.log")]
    log_file: String,
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
        "aaos-sentinel starting, socket: {}, log: {}",
        args.socket, args.log_file
    );

    let listener = listen(&args.socket).await?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            warn!("aaos-sentinel received SIGINT, shutting down");
        }
        res = run(listener, &args.log_file) => {
            if let Err(e) = res {
                tracing::error!("sentinel run error: {e:#}");
            }
        }
    }

    Ok(())
}

async fn run(listener: tokio::net::UnixListener, log_file: &str) -> Result<()> {
    let log_file = log_file.to_string();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let log_file = log_file.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_socket_connection(stream, &log_file).await {
                        tracing::warn!("connection error: {e:#}");
                    }
                });
            }
            Err(e) => {
                tracing::error!("accept error: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn handle_socket_connection(
    stream: tokio::net::UnixStream,
    log_file: &str,
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

        if let Message::Event {
            timestamp,
            source,
            severity,
            event_type,
            payload,
        } = msg
        {
            log_event(log_file, &timestamp, source, severity, &event_type, &payload).await;
        }

        let ack = Message::Ack {
            timestamp: chrono::Utc::now().to_rfc3339(),
            result: Some("logged".into()),
        };
        let _ = write_message(&mut write, &ack).await;
    }
}

async fn log_event(
    log_file: &str,
    timestamp: &str,
    source: EventSource,
    severity: Severity,
    event_type: &str,
    payload: &serde_json::Value,
) {
    let line = format!(
        "[{timestamp}] {source:?}/{severity:?} {event_type}: {payload}\n",
        source = source,
        severity = severity,
        event_type = event_type,
        payload = payload,
    );
    tracing::info!("{}", line.trim_end());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}
