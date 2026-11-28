//! The live channel: Supabase Realtime (the Phoenix protocol over a WebSocket). Each
//! account listens on its private Broadcast channel "clips:<user id>", where the
//! database pushes new, bumped and deleted clips.
//!
//! One heartbeat every 30 seconds keeps the connection open; nothing else runs while
//! no clips arrive. The app closes the connection while the PC is locked or asleep.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, interval_at};
use tokio_tungstenite::tungstenite::Message;

use crate::error::Error;
use crate::model::Clip;

const HEARTBEAT: Duration = Duration::from_secs(30);
const JOIN_REF: &str = "1";

#[derive(Debug, Clone, PartialEq)]
pub enum RtEvent {
    /// Listening: anything that changed before this has to be fetched (catch-up).
    Joined,
    Upsert(Box<Clip>),
    Delete(String),
}

/// A message from the server, as far as Smart Clip cares.
#[derive(Debug, PartialEq)]
enum Incoming {
    Reply {
        reference: String,
        ok: bool,
        message: String,
    },
    Event(RtEvent),
    /// The server closed the channel or reported a channel error.
    Closed(String),
    Other,
}

fn reply_message(payload: &Value) -> String {
    let r = &payload["response"];
    r["reason"].as_str().or_else(|| r["message"].as_str()).or_else(|| r.as_str()).unwrap_or_default().to_owned()
}

fn parse(text: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Incoming::Other };
    let event = v["event"].as_str().unwrap_or_default();
    let payload = &v["payload"];
    match event {
        "phx_reply" => Incoming::Reply {
            reference: v["ref"].as_str().unwrap_or_default().to_owned(),
            ok: payload["status"].as_str() == Some("ok"),
            message: reply_message(payload),
        },
        "broadcast" => {
            let inner = &payload["payload"];
            match payload["event"].as_str().unwrap_or_default() {
                "INSERT" | "UPDATE" => match serde_json::from_value::<Clip>(inner.clone()) {
                    Ok(clip) => Incoming::Event(RtEvent::Upsert(Box::new(clip))),
                    Err(_) => Incoming::Other,
                },
                "DELETE" => match inner["id"].as_str() {
                    Some(id) => Incoming::Event(RtEvent::Delete(id.to_owned())),
                    None => Incoming::Other,
                },
                _ => Incoming::Other,
            }
        }
        "system" if payload["status"].as_str() == Some("error") => {
            Incoming::Closed(payload["message"].as_str().unwrap_or("channel error").to_owned())
        }
        "phx_error" | "phx_close" => Incoming::Closed(event.to_owned()),
        _ => Incoming::Other,
    }
}

fn frame(topic: &str, event: &str, payload: Value, reference: &str) -> Message {
    let join_ref = if topic == "phoenix" { Value::Null } else { Value::from(JOIN_REF) };
    Message::text(
        json!({ "topic": topic, "event": event, "payload": payload, "ref": reference, "join_ref": join_ref })
            .to_string(),
    )
}

fn is_auth_problem(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("unauthorized") || m.contains("token") || m.contains("jwt") || m.contains("expired")
}

/// Resolves once `stop` is true (or its sender is gone).
async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// Listens until `stop` turns true (returns Ok) or the connection drops (returns the
/// reason). New access tokens arrive through `token` and are passed on to the server.
pub async fn run(
    url: &str,
    user_id: &str,
    mut token: watch::Receiver<String>,
    events: mpsc::UnboundedSender<RtEvent>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), Error> {
    let topic = format!("realtime:clips:{user_id}");
    let connect = tokio_tungstenite::connect_async(url);
    let (mut ws, _) = tokio::select! {
        r = tokio::time::timeout(Duration::from_secs(20), connect) => match r {
            Ok(Ok(c)) => c,
            _ => return Err(Error::Offline),
        },
        _ = stopped(&mut stop) => return Ok(()),
    };

    let access = token.borrow_and_update().clone();
    let join = json!({
        "config": {
            "broadcast": { "ack": false, "self": false },
            "presence": { "enabled": false },
            "postgres_changes": [],
            "private": true
        },
        "access_token": access
    });
    ws.send(frame(&topic, "phx_join", join, JOIN_REF)).await.map_err(|_| Error::Offline)?;

    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    let mut next_ref: u64 = 2;
    let mut waiting_heartbeat: Option<String> = None;

    loop {
        tokio::select! {
            msg = ws.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(p))) => {
                        let _ = ws.send(Message::Pong(p)).await;
                        continue;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return Err(Error::Offline),
                    Some(Ok(_)) => continue,
                };
                match parse(text.as_str()) {
                    Incoming::Reply { reference, ok, message } => {
                        if reference == JOIN_REF {
                            if !ok {
                                return Err(if is_auth_problem(&message) {
                                    Error::SignedOut
                                } else {
                                    Error::Server { status: 0, message: format!("live updates: {message}") }
                                });
                            }
                            let _ = events.send(RtEvent::Joined);
                        } else if waiting_heartbeat.as_deref() == Some(reference.as_str()) {
                            waiting_heartbeat = None;
                        }
                    }
                    Incoming::Event(e) => {
                        let _ = events.send(e);
                    }
                    Incoming::Closed(reason) => {
                        return Err(if is_auth_problem(&reason) { Error::SignedOut } else { Error::Offline });
                    }
                    Incoming::Other => {}
                }
            }
            _ = heartbeat.tick() => {
                // No answer to the last heartbeat in 30 seconds: the connection is dead
                // (common after Wi-Fi changes), so start a new one.
                if waiting_heartbeat.is_some() {
                    return Err(Error::Offline);
                }
                let r = next_ref.to_string();
                next_ref += 1;
                ws.send(frame("phoenix", "heartbeat", json!({}), &r)).await.map_err(|_| Error::Offline)?;
                waiting_heartbeat = Some(r);
            }
            changed = token.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let access = token.borrow_and_update().clone();
                let r = next_ref.to_string();
                next_ref += 1;
                ws.send(frame(&topic, "access_token", json!({ "access_token": access }), &r))
                    .await
                    .map_err(|_| Error::Offline)?;
            }
            _ = stopped(&mut stop) => {
                let _ = ws.send(frame(&topic, "phx_leave", json!({}), &next_ref.to_string())).await;
                let _ = ws.close(None).await;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: &str = r#"{"id":"0b0e8c0e-6f0e-4a5e-9a7b-1c2d3e4f5a6b","user_id":"u","device_id":"d","device_name":"Desk PC","kind":"text","hash":"ab","text":"hello","thumb":null,"image_path":null,"width":null,"height":null,"bytes":0,"created_at":"2026-11-28T10:00:00.123456+00:00","expires_at":"2026-12-05T10:00:00.123456+00:00"}"#;

    #[test]
    fn reads_broadcast_inserts() {
        let msg = format!(
            r#"{{"event":"broadcast","payload":{{"event":"INSERT","payload":{ROW},"type":"broadcast","meta":{{"id":"x"}}}},"ref":null,"topic":"realtime:clips:u"}}"#
        );
        match parse(&msg) {
            Incoming::Event(RtEvent::Upsert(c)) => {
                assert_eq!(c.text.as_deref(), Some("hello"));
                assert_eq!(c.device_name, "Desk PC");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_broadcast_deletes() {
        let msg = r#"{"event":"broadcast","payload":{"event":"DELETE","payload":{"id":"abc"},"type":"broadcast"},"ref":null,"topic":"t"}"#;
        assert_eq!(parse(msg), Incoming::Event(RtEvent::Delete("abc".into())));
    }

    #[test]
    fn reads_replies_and_errors() {
        let ok = r#"{"event":"phx_reply","payload":{"status":"ok","response":{}},"ref":"1","topic":"t"}"#;
        assert_eq!(parse(ok), Incoming::Reply { reference: "1".into(), ok: true, message: String::new() });
        let bad = r#"{"event":"phx_reply","payload":{"status":"error","response":{"reason":"Unauthorized: You do not have permissions"}},"ref":"1","topic":"t"}"#;
        match parse(bad) {
            Incoming::Reply { ok: false, message, .. } => assert!(is_auth_problem(&message)),
            other => panic!("{other:?}"),
        }
        let sys = r#"{"event":"system","payload":{"status":"error","message":"Token has expired 5 seconds ago"},"ref":null,"topic":"t"}"#;
        assert!(matches!(parse(sys), Incoming::Closed(m) if is_auth_problem(&m)));
        assert_eq!(parse("not json"), Incoming::Other);
    }

    #[test]
    fn frames() {
        let Message::Text(t) = frame("phoenix", "heartbeat", json!({}), "7") else { panic!() };
        let v: Value = serde_json::from_str(t.as_str()).unwrap();
        assert_eq!(v["ref"], "7");
        assert!(v["join_ref"].is_null());
        let Message::Text(t) = frame("realtime:clips:u", "phx_join", json!({}), "1") else { panic!() };
        let v: Value = serde_json::from_str(t.as_str()).unwrap();
        assert_eq!(v["join_ref"], "1");
    }
}
