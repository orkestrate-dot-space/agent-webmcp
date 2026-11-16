//! A minimal Chrome DevTools Protocol client: one WebSocket to the browser,
//! flattened sessions, commands with replies, and a stream of events.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// A CDP event, tagged with the session it belongs to.
#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub params: Value,
    pub session_id: Option<String>,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

#[derive(Clone)]
pub struct Client {
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    pending: Pending,
    events: broadcast::Sender<Event>,
    next_id: Arc<AtomicU64>,
}

impl Client {
    /// Connect to a browser-level WebSocket URL (`ws://127.0.0.1:9222/devtools/browser/...`).
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .with_context(|| format!("connecting to {ws_url}"))?;
        let (mut sink, mut source) = stream.split();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let pending: Pending = Arc::default();
        let (events, _) = broadcast::channel(4096);

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let pending_reader = pending.clone();
        let events_reader = events.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = source.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(id) = v.get("id").and_then(Value::as_u64) {
                    if let Some(reply) = pending_reader.lock().await.remove(&id) {
                        let result = match v.get("error") {
                            Some(e) => Err(anyhow!(
                                "{}",
                                e.get("message").and_then(Value::as_str).unwrap_or("CDP error")
                            )),
                            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = reply.send(result);
                    }
                } else if let Some(method) = v.get("method").and_then(Value::as_str) {
                    let _ = events_reader.send(Event {
                        method: method.to_string(),
                        params: v.get("params").cloned().unwrap_or(Value::Null),
                        session_id: v.get("sessionId").and_then(Value::as_str).map(str::to_string),
                    });
                }
            }
            // The socket closed: fail everything still waiting.
            for (_, reply) in pending_reader.lock().await.drain() {
                let _ = reply.send(Err(anyhow!("the browser connection closed")));
            }
        });

        Ok(Self { tx, pending, events, next_id: Arc::new(AtomicU64::new(1)) })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Send a command and wait for its reply.
    pub async fn send(&self, method: &str, params: Value, session: Option<&str>, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.pending.lock().await.insert(id, reply_tx);
        if self.tx.send(Message::Text(msg.to_string().into())).is_err() {
            self.pending.lock().await.remove(&id);
            bail!("the browser connection is closed");
        }
        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(result)) => result.with_context(|| method.to_string()),
            Ok(Err(_)) => bail!("{method}: no reply"),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                bail!("{method}: timed out after {}s", timeout.as_secs())
            }
        }
    }
}
