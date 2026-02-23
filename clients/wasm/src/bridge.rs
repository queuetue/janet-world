//! WASM bridge — WebSocket connection to the NATS server.
//!
//! ## Threading model
//!
//! WASM is single-threaded.  We use `wasm_bindgen_futures::spawn_local`
//! instead of OS threads, and `Rc<RefCell<…>>` instead of `Arc<Mutex<…>>`.
//!
//! ```text
//! JS main frame
//! ─────────────────────────────────────────────────────────
//! JanetWorldClient::poll()
//!   drains state.events  →  fires JS callbacks
//!   queues state.intents →  picked up by bridge loop
//!
//! wasm_bindgen_futures::spawn_local (cooperative future)
//!   BridgeLoop::tick() every ~8 ms
//!     drain state.incoming (raw NATS text frames from WebSocket)
//!     → parse_frame() → NatsOp → WorldEvent → state.events
//!     drain state.intents → ws.send_with_str(PUB frame)
//!
//! web_sys::WebSocket callbacks (onmessage / onopen / onclose / onerror)
//!   push raw frames into state.incoming
//!   mark state.ws_open / state.ws_closed
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::events::WorldEvent;
use crate::nats_ws::{self, NatsOp};

// ---------------------------------------------------------------------------
// Shared state (single-threaded, Rc/RefCell)
// ---------------------------------------------------------------------------

struct State {
    /// Raw NATS text frames received from the WebSocket.
    incoming: VecDeque<String>,
    /// Serialised NATS PUB frames to send next tick.
    intents: VecDeque<String>,
    /// Processed WorldEvents waiting for JS poll().
    pub events: VecDeque<WorldEvent>,
    /// Set to true when the WebSocket `onopen` fires.
    ws_open: bool,
    /// Set to true on `onclose` / `onerror`.
    ws_closed: bool,
    /// Close reason if `ws_closed`.
    close_reason: String,
    /// True after we have sent the CONNECT + SUB frames.
    subscribed: bool,
}

impl State {
    fn new() -> Self {
        Self {
            incoming: VecDeque::new(),
            intents: VecDeque::new(),
            events: VecDeque::new(),
            ws_open: false,
            ws_closed: false,
            close_reason: String::new(),
            subscribed: false,
        }
    }
}

type SharedState = Rc<RefCell<State>>;

// ---------------------------------------------------------------------------
// BridgeConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// WebSocket URL for the NATS server (e.g. `wss://nats01.internal.plantange.net/`).
    pub endpoint: String,
    /// Janet session name.
    pub session: String,
    /// Participant ID advertised on the bus.
    pub participant_id: String,
    /// How many Events to buffer before dropping oldest (back-pressure).
    pub event_buffer: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            endpoint: "wss://nats01.internal.plantange.net/".into(),
            session: "default".into(),
            participant_id: "wasm-client".into(),
            event_buffer: 1024,
        }
    }
}

// ---------------------------------------------------------------------------
// Bridge handle (owned by JanetWorldClient)
// ---------------------------------------------------------------------------

/// Returned by [`spawn`] — allows `JanetWorldClient` to interact with the
/// bridge from JS poll calls.
pub struct BridgeHandle {
    shared: SharedState,
    ws: web_sys::WebSocket,
    // Keep closures alive for the lifetime of the bridge.
    _onopen: Closure<dyn FnMut()>,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _onerror: Closure<dyn FnMut(web_sys::Event)>,
    _onclose: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

impl BridgeHandle {
    /// Drain up to `limit` pending world events.  Call from JS poll().
    pub fn drain_events(&self, limit: usize) -> Vec<WorldEvent> {
        let mut st = self.shared.borrow_mut();
        let n = limit.min(st.events.len());
        st.events.drain(..n).collect()
    }

    /// Queue an outbound NATS PUB frame.
    pub fn queue_intent(&self, pub_frame: String) {
        self.shared.borrow_mut().intents.push_back(pub_frame);
    }

    /// True if the WebSocket is still alive.
    pub fn is_alive(&self) -> bool {
        let st = self.shared.borrow();
        !st.ws_closed
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onerror(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Open a WebSocket to the NATS server, start the bridge loop, and return
/// a [`BridgeHandle`] for the JS layer.
pub fn spawn(config: BridgeConfig) -> Result<BridgeHandle, JsValue> {
    let shared: SharedState = Rc::new(RefCell::new(State::new()));

    let ws = web_sys::WebSocket::new(&config.endpoint)?;
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // ----- onopen -----------------------------------------------------------
    let shared_open = shared.clone();
    let config_open = config.clone();
    let ws_open = ws.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        log::info!("[bridge] WebSocket open — sending CONNECT");
        shared_open.borrow_mut().ws_open = true;
        let frame = nats_ws::connect_frame(&config_open.participant_id);
        let _ = ws_open.send_with_str(&frame);
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

    // ----- onmessage --------------------------------------------------------
    let shared_msg = shared.clone();
    let onmessage =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
            if let Some(text) = ev.data().as_string() {
                shared_msg.borrow_mut().incoming.push_back(text);
            }
        });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    // ----- onerror ----------------------------------------------------------
    let shared_err = shared.clone();
    let onerror = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
        let msg = "WebSocket error";
        log::error!("[bridge] {}", msg);
        let mut st = shared_err.borrow_mut();
        st.ws_closed = true;
        st.close_reason = msg.to_string();
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

    // ----- onclose ----------------------------------------------------------
    let shared_close = shared.clone();
    let onclose = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |ev: web_sys::CloseEvent| {
        log::info!("[bridge] WebSocket closed: {}", ev.reason());
        let mut st = shared_close.borrow_mut();
        st.ws_open = false;
        st.ws_closed = true;
        st.close_reason = ev.reason();
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

    // ----- Spawn bridge loop ------------------------------------------------
    let shared_loop = shared.clone();
    let config_loop = config.clone();
    let ws_loop = ws.clone();
    wasm_bindgen_futures::spawn_local(async move {
        run_bridge_loop(shared_loop, config_loop, ws_loop).await;
    });

    Ok(BridgeHandle {
        shared,
        ws,
        _onopen: onopen,
        _onmessage: onmessage,
        _onerror: onerror,
        _onclose: onclose,
    })
}

// ---------------------------------------------------------------------------
// Async bridge loop (runs as a cooperative task)
// ---------------------------------------------------------------------------

async fn run_bridge_loop(shared: SharedState, config: BridgeConfig, ws: web_sys::WebSocket) {
    loop {
        sleep_ms(8).await;

        let closed = {
            let st = shared.borrow();
            st.ws_closed
        };
        if closed {
            let reason = shared.borrow().close_reason.clone();
            shared
                .borrow_mut()
                .events
                .push_back(WorldEvent::Disconnected {
                    reason: if reason.is_empty() {
                        "WebSocket closed".into()
                    } else {
                        reason
                    },
                });
            break;
        }

        // Drain incoming NATS frames
        let frames: Vec<String> = {
            let mut st = shared.borrow_mut();
            st.incoming.drain(..).collect()
        };

        for frame in frames {
            let ops = nats_ws::parse_frame(&frame);
            process_ops(ops, &shared, &config, &ws);
        }

        // Drain outbound intent queue
        let intents: Vec<String> = {
            let mut st = shared.borrow_mut();
            st.intents.drain(..).collect()
        };
        for pub_frame in intents {
            if let Err(e) = ws.send_with_str(&pub_frame) {
                log::warn!("[bridge] Failed to send intent: {:?}", e);
            }
        }
    }

    log::info!("[bridge] Loop exited");
}

// ---------------------------------------------------------------------------
// NATS op processing
// ---------------------------------------------------------------------------

fn process_ops(
    ops: Vec<NatsOp>,
    shared: &SharedState,
    config: &BridgeConfig,
    ws: &web_sys::WebSocket,
) {
    for op in ops {
        match op {
            NatsOp::Info { .. } => {
                // INFO is only sent once, on connect, before we send CONNECT.
                // If we see it again (e.g. after TLS upgrade), re-subscribe.
                let already = shared.borrow().subscribed;
                if !already {
                    // Subscribe to all world.> events
                    let sub = nats_ws::sub_frame("world.>", 1);
                    let _ = ws.send_with_str(&sub);
                    shared.borrow_mut().subscribed = true;

                    // Announce participant join
                    let join = serde_json::json!({
                        "id": config.participant_id,
                        "x": 0.0, "y": 0.0, "z": 0.0
                    });
                    let _ = ws.send_with_str(&nats_ws::pub_frame(
                        "world.participant.join",
                        &join.to_string(),
                    ));

                    // Emit Connected event
                    shared.borrow_mut().events.push_back(WorldEvent::Connected {
                        session: config.session.clone(),
                        participant_id: config.participant_id.clone(),
                        frame: 0,
                    });

                    log::info!("[bridge] Subscribed to world.>");
                }
            }

            NatsOp::Ping => {
                let _ = ws.send_with_str(&nats_ws::pong_frame());
            }

            NatsOp::Msg { subject, payload } => {
                let events = parse_nats_message(&subject, &payload, shared.borrow().events.len());
                let mut st = shared.borrow_mut();
                for ev in events {
                    if st.events.len() < 1024 {
                        st.events.push_back(ev);
                    } else {
                        log::warn!("[bridge] Event queue full — dropping {:?}", subject);
                    }
                }
            }

            NatsOp::Err { message } => {
                log::error!("[bridge] NATS error: {}", message);
            }

            NatsOp::Ok => {}
        }
    }
}

// ---------------------------------------------------------------------------
// NATS message → WorldEvent  (same logic as the Godot bridge.rs)
// ---------------------------------------------------------------------------

fn parse_nats_message(subject: &str, payload: &[u8], _queue_len: usize) -> Vec<WorldEvent> {
    let v: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[bridge] Bad JSON on {}: {}", subject, e);
            return vec![];
        }
    };

    let frame = v.get("frame").and_then(|f| f.as_u64()).unwrap_or(0);
    let inner = v.get("payload").unwrap_or(&v);

    match subject {
        s if s.starts_with("world.chunk.activated") => vec![WorldEvent::ChunkActivated {
            chunk_id: str_field(inner, "chunk_id"),
            cx: int_field(inner, "cx"),
            cy: int_field(inner, "cy"),
            seed: inner.get("seed").and_then(|v| v.as_u64()).unwrap_or(0),
            lod: inner.get("lod").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
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
    }
}

// ---------------------------------------------------------------------------
// Helper: async sleep via setTimeout
// ---------------------------------------------------------------------------

/// Yield to the JS event loop for `ms` milliseconds.
///
/// This is the WASM equivalent of `tokio::time::sleep`.  We wrap a
/// `setTimeout` `Promise` and `await` it.
async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no global window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32)
            .expect("setTimeout failed");
    });
    let _ = JsFuture::from(promise).await;
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|f| f.as_str())
        .unwrap_or("")
        .to_string()
}

fn int_field(v: &serde_json::Value, key: &str) -> i32 {
    v.get(key).and_then(|f| f.as_i64()).unwrap_or(0) as i32
}

fn float_field(v: &serde_json::Value, key: &str) -> f32 {
    v.get(key).and_then(|f| f.as_f64()).unwrap_or(0.0) as f32
}
