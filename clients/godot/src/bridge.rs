//! Bus bridge – Tokio thread connecting to the janet bus, delivering events.
//!
//! ## Threading model
//!
//! ```text
//! Godot main thread          │  Bridge thread (Tokio)
//! ───────────────────────── │ ─────────────────────────
//! JanetWorldClient::_process │ BusBridge::run()
//!   → rx.try_recv()         │   subscribe world.*
//!   → cache.apply(event)    │   → tx.send(WorldEvent)
//!   → emit_signal(...)      │
//!                            │
//!   send_intent(...)         │
//!   → intent_tx.send(msg)   │   intent_rx.recv()
//!                            │   → client.publish(...)
//! ```
//!
//! The bridge thread owns the NATS connection and the Tokio runtime.
//! The Godot thread never touches async code — it only reads from
//! `crossbeam_channel` receivers.

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use serde_json::Value;
use std::thread;

use crate::events::WorldEvent;

// ---------------------------------------------------------------------------
// Intent (Godot thread → bridge thread)
// ---------------------------------------------------------------------------

/// An intent sent from the Godot thread to the bridge.
#[derive(Debug, Clone)]
pub enum IntentMessage {
    Move { dx: f32, dy: f32, dz: f32 },
    Interact { target_id: String, verb: Option<String> },
    Teleport { x: f32, y: f32, z: f32 },
    ViewRadius { radius: f32 },
    RequestSnapshot { x: f32, y: f32, z: f32, radius: f32 },
    Disconnect,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// NATS endpoint (e.g. "nats://localhost:4222")
    pub endpoint: String,
    /// Session to join
    pub session: String,
    /// Participant ID this client uses on the bus
    pub participant_id: String,
    /// How deep to buffer world events before dropping (back-pressure)
    pub event_buffer: usize,
    /// How deep to buffer outbound intents
    pub intent_buffer: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            endpoint: "nats://localhost:4222".into(),
            session: "default".into(),
            participant_id: "godot-client".into(),
            event_buffer: 1024,
            intent_buffer: 64,
        }
    }
}

// ---------------------------------------------------------------------------
// Handle (given to Godot thread)
// ---------------------------------------------------------------------------

/// Owned by the Godot-side `JanetWorldClient` node.
pub struct BusHandle {
    /// Receive processed world events (non-blocking).
    pub events: Receiver<WorldEvent>,
    /// Send outbound intent messages to the bridge thread.
    pub intents: Sender<IntentMessage>,
    /// Join handle — kept alive for the duration.
    _thread: thread::JoinHandle<()>,
}

impl BusHandle {
    /// Drain all pending events without blocking.
    pub fn poll(&self) -> Vec<WorldEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(ev) => out.push(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Send an intent (fire-and-forget, drops if channel is full).
    pub fn send_intent(&self, intent: IntentMessage) {
        let _ = self.intents.try_send(intent);
    }
}

// ---------------------------------------------------------------------------
// Spawning the bridge thread
// ---------------------------------------------------------------------------

/// Spawn the bridge thread and return a [`BusHandle`] for the Godot thread.
pub fn spawn(config: BridgeConfig) -> BusHandle {
    let (event_tx, event_rx) = crossbeam_channel::bounded::<WorldEvent>(config.event_buffer);
    let (intent_tx, intent_rx) =
        crossbeam_channel::bounded::<IntentMessage>(config.intent_buffer);

    let handle = thread::Builder::new()
        .name("janet-world-bridge".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create bridge Tokio runtime");

            rt.block_on(run_bridge(config, event_tx, intent_rx));
        })
        .expect("Failed to spawn janet-world bridge thread");

    BusHandle {
        events: event_rx,
        intents: intent_tx,
        _thread: handle,
    }
}

// ---------------------------------------------------------------------------
// Async bridge implementation
// ---------------------------------------------------------------------------

async fn run_bridge(
    config: BridgeConfig,
    event_tx: Sender<WorldEvent>,
    intent_rx: Receiver<IntentMessage>,
) {
    log::info!(
        "[bridge] Connecting to {} (session={})",
        config.endpoint,
        config.session
    );

    let nc = match async_nats::connect(&config.endpoint).await {
        Ok(c) => c,
        Err(e) => {
            log::error!("[bridge] NATS connect failed: {}", e);
            let _ = event_tx.try_send(WorldEvent::Disconnected {
                reason: format!("NATS connect failed: {}", e),
            });
            return;
        }
    };

    log::info!("[bridge] Connected");

    // Announce our presence (world.participant.join)
    let join_payload = serde_json::json!({
        "id": config.participant_id,
        "x": 0.0, "y": 0.0, "z": 0.0
    });
    let _ = nc
        .publish(
            "world.participant.join",
            join_payload.to_string().into(),
        )
        .await;

    let _ = event_tx.try_send(WorldEvent::Connected {
        session: config.session.clone(),
        participant_id: config.participant_id.clone(),
        frame: 0,
    });

    // Subscribe to all world.* subjects
    let mut world_sub = match nc.subscribe("world.>").await {
        Ok(s) => s,
        Err(e) => {
            log::error!("[bridge] Subscribe failed: {}", e);
            return;
        }
    };

    log::info!("[bridge] Subscribed to world.>");

    // Main loop: drive subscriptions and outbound intents concurrently
    loop {
        tokio::select! {
            // Inbound: bus message from server
            msg = world_sub.next() => {
                let Some(msg) = msg else { break };
                if let Ok(events) = parse_bus_message(&msg.subject, &msg.payload) {
                    for ev in events {
                        if event_tx.try_send(ev).is_err() {
                            log::warn!("[bridge] Event channel full – dropping event");
                        }
                    }
                }
            }

            // Outbound: intent from Godot thread
            _ = poll_intents(&nc, &intent_rx, &config) => {}
        }
    }

    log::info!("[bridge] Disconnected — exiting run loop");
    let _ = event_tx.try_send(WorldEvent::Disconnected {
        reason: "server closed connection".into(),
    });
}

/// Drain the intent channel and publish one batch to NATS.
async fn poll_intents(
    nc: &async_nats::Client,
    rx: &Receiver<IntentMessage>,
    config: &BridgeConfig,
) {
    // Yield briefly so the select can make progress
    tokio::time::sleep(tokio::time::Duration::from_millis(8)).await;

    while let Ok(intent) = rx.try_recv() {
        let (subject, payload) = intent_to_bus(&config.participant_id, intent);
        let _ = nc.publish(subject, payload.into()).await;
    }
}

// ---------------------------------------------------------------------------
// Message parsing: bus bytes → WorldEvent list
// ---------------------------------------------------------------------------

fn parse_bus_message(
    subject: &str,
    payload: &[u8],
) -> Result<Vec<WorldEvent>, serde_json::Error> {
    let v: Value = serde_json::from_slice(payload)?;

    // Strip the optional WorldEvent envelope (frame + session + payload)
    let frame = v.get("frame").and_then(|f| f.as_u64()).unwrap_or(0);
    let inner = v.get("payload").unwrap_or(&v);

    let events = match subject {
        s if s.starts_with("world.chunk.activated") => vec![WorldEvent::ChunkActivated {
            chunk_id: str_field(inner, "chunk_id"),
            cx: int_field(inner, "cx"),
            cy: int_field(inner, "cy"),
            seed: inner.get("seed").and_then(|v| v.as_u64()).unwrap_or(0),
            lod: inner
                .get("lod")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8,
            chunk_size: float_field(inner, "chunk_size"),
        }],

        s if s.starts_with("world.chunk.deactivated") => vec![WorldEvent::ChunkDeactivated {
            chunk_id: str_field(inner, "chunk_id"),
        }],

        s if s.starts_with("world.structure.spawned") => vec![WorldEvent::StructureSpawned {
            structure_id: str_field(inner, "structure_id"),
            type_id: str_field(inner, "type_id"),
            x: float_field(inner, "x"),
            y: float_field(inner, "y"),
            z: float_field(inner, "z"),
            rotation_y: float_field(inner, "rotation_y"),
        }],

        s if s.starts_with("world.structure.removed") => vec![WorldEvent::StructureRemoved {
            structure_id: str_field(inner, "structure_id"),
        }],

        s if s.starts_with("world.entity.spawned") => vec![WorldEvent::EntitySpawned {
            entity_id: str_field(inner, "entity_id"),
            archetype: str_field(inner, "archetype"),
            x: float_field(inner, "x"),
            y: float_field(inner, "y"),
            z: float_field(inner, "z"),
            rotation_y: float_field(inner, "rotation_y"),
        }],

        s if s.starts_with("world.entity.removed") => vec![WorldEvent::EntityRemoved {
            entity_id: str_field(inner, "entity_id"),
        }],

        s if s.starts_with("world.entity.transform") => vec![WorldEvent::EntityTransform {
            entity_id: str_field(inner, "entity_id"),
            x: float_field(inner, "x"),
            y: float_field(inner, "y"),
            z: float_field(inner, "z"),
            rotation_y: float_field(inner, "rotation_y"),
            vx: float_field(inner, "vx"),
            vy: float_field(inner, "vy"),
            vz: float_field(inner, "vz"),
            frame,
            dt: float_field(inner, "dt"),
        }],

        s if s.starts_with("world.snapshot") => {
            // Snapshot is handled inline – expand into constituent events
            let mut evs = vec![WorldEvent::SnapshotBegin { frame }];

            if let Some(chunks) = inner.get("active_chunks").and_then(|v| v.as_array()) {
                for c in chunks {
                    evs.push(WorldEvent::ChunkActivated {
                        chunk_id: str_field(c, "chunk_id"),
                        cx: int_field(c, "cx"),
                        cy: int_field(c, "cy"),
                        seed: c.get("seed").and_then(|v| v.as_u64()).unwrap_or(0),
                        lod: c.get("lod").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                        chunk_size: float_field(c, "chunk_size"),
                    });
                }
            }
            if let Some(structures) = inner.get("structures").and_then(|v| v.as_array()) {
                for s in structures {
                    evs.push(WorldEvent::StructureSpawned {
                        structure_id: str_field(s, "structure_id"),
                        type_id: str_field(s, "type_id"),
                        x: float_field(s, "x"),
                        y: float_field(s, "y"),
                        z: float_field(s, "z"),
                        rotation_y: float_field(s, "rotation_y"),
                    });
                }
            }
            if let Some(entities) = inner.get("entities").and_then(|v| v.as_array()) {
                for e in entities {
                    evs.push(WorldEvent::EntitySpawned {
                        entity_id: str_field(e, "entity_id"),
                        archetype: str_field(e, "archetype"),
                        x: float_field(e, "x"),
                        y: float_field(e, "y"),
                        z: float_field(e, "z"),
                        rotation_y: float_field(e, "rotation_y"),
                    });
                }
            }
            evs.push(WorldEvent::SnapshotEnd);
            evs
        }

        _ => vec![],
    };

    Ok(events)
}

// ---------------------------------------------------------------------------
// Intent serialisation
// ---------------------------------------------------------------------------

fn intent_to_bus(participant_id: &str, intent: IntentMessage) -> (String, String) {
    match intent {
        IntentMessage::Move { dx, dy, dz } => (
            "intent.move".into(),
            serde_json::json!({ "id": participant_id, "dx": dx, "dy": dy, "dz": dz })
                .to_string(),
        ),
        IntentMessage::Interact { target_id, verb } => (
            "intent.interact".into(),
            serde_json::json!({ "id": participant_id, "target_id": target_id, "verb": verb })
                .to_string(),
        ),
        IntentMessage::Teleport { x, y, z } => (
            "intent.teleport".into(),
            serde_json::json!({ "id": participant_id, "x": x, "y": y, "z": z }).to_string(),
        ),
        IntentMessage::ViewRadius { radius } => (
            "intent.view_radius".into(),
            serde_json::json!({ "id": participant_id, "radius": radius }).to_string(),
        ),
        IntentMessage::RequestSnapshot { x, y, z, radius } => (
            "world.cmd.snapshot".into(),
            serde_json::json!({ "id": participant_id, "x": x, "y": y, "z": z, "radius": radius })
                .to_string(),
        ),
        IntentMessage::Disconnect => (
            "world.participant.leave".into(),
            serde_json::json!({ "id": participant_id }).to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|f| f.as_str())
        .unwrap_or("")
        .to_string()
}

fn int_field(v: &Value, key: &str) -> i32 {
    v.get(key).and_then(|f| f.as_i64()).unwrap_or(0) as i32
}

fn float_field(v: &Value, key: &str) -> f32 {
    v.get(key).and_then(|f| f.as_f64()).unwrap_or(0.0) as f32
}
