//! IPC 协议：基于 Unix Socket 的 JSON line-based 通信
//!
//! 三个组件交互：
//! - CLI → Core Agent：自然语言请求 / 响应
//! - Core Agent → Sentinel：日志事件
//!
//! 线协议格式：每条消息是单行 JSON，以 `\n` 结束。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// IPC 消息（用 `type` 字段做 tag）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// CLI → Core：用户请求
    Request {
        id: String,
        input: String,
        /// 用户对破坏性命令的确认 token
        confirmation_token: Option<String>,
    },
    /// Core → CLI：响应
    Response {
        id: String,
        status: ResponseStatus,
        output: String,
        /// 当 status == NeedsConfirmation 时携带，用户带回以确认
        confirmation_token: Option<String>,
    },
    /// Core → Sentinel：日志事件
    Event {
        timestamp: String,
        source: EventSource,
        severity: Severity,
        event_type: String,
        payload: Value,
    },
    /// Sentinel → Core / CLI：ack（可选）
    Ack {
        timestamp: String,
        #[serde(default)]
        result: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Error,
    NeedsConfirmation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Ai,
    Zfs,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Message {
    /// 序列化为一行 JSON（带换行）。
    pub fn encode(&self) -> Result<String> {
        let mut s = serde_json::to_string(self).context("serialize message")?;
        s.push('\n');
        Ok(s)
    }

    /// 从单行 JSON 反序列化。
    pub fn decode(line: &str) -> Result<Self> {
        serde_json::from_str(line.trim()).context("deserialize message")
    }
}

/// 起一个 Unix Socket 服务器。绑定前会清理已有 socket 文件。
pub async fn listen(path: impl AsRef<Path>) -> Result<UnixListener> {
    let path = path.as_ref();
    if path.exists() {
        std::fs::remove_file(path).context("remove stale socket")?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create socket parent dir")?;
    }
    let listener = UnixListener::bind(path).context("bind unix socket")?;
    tracing::info!("listening on {}", path.display());
    Ok(listener)
}

/// 连接到 Unix Socket。
pub async fn connect(path: impl AsRef<Path>) -> Result<UnixStream> {
    UnixStream::connect(path).await.context("connect unix socket")
}

/// 从流里读一行消息。
pub async fn read_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<Option<Message>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await.context("read line")?;
    if n == 0 {
        return Ok(None);
    }
    Message::decode(&line).map(Some)
}

/// 把消息写入流（带换行）。
pub async fn write_message<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &Message) -> Result<()> {
    let s = msg.encode()?;
    writer.write_all(s.as_bytes()).await.context("write message")?;
    writer.flush().await.context("flush")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_encode_decode() {
        let m = Message::Request {
            id: "abc".into(),
            input: "看看池状态".into(),
            confirmation_token: None,
        };
        let s = m.encode().unwrap();
        let m2 = Message::decode(&s).unwrap();
        match m2 {
            Message::Request { id, input, .. } => {
                assert_eq!(id, "abc");
                assert_eq!(input, "看看池状态");
            }
            _ => panic!("wrong type"),
        }
    }

    #[test]
    fn event_encode_decode() {
        let m = Message::Event {
            timestamp: "2026-07-23T00:00:00Z".into(),
            source: EventSource::Ai,
            severity: Severity::Info,
            event_type: "ai.action".into(),
            payload: serde_json::json!({"action": "zpool list"}),
        };
        let s = m.encode().unwrap();
        let m2 = Message::decode(&s).unwrap();
        match m2 {
            Message::Event { source, .. } => assert_eq!(source, EventSource::Ai),
            _ => panic!("wrong type"),
        }
    }
}
