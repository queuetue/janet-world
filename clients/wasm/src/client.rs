//! `JanetWorldClient` — the primary wasm-bindgen export.
//!
//! ## JavaScript usage
//!
//! ```js
//! import init, { JanetWorldClient } from './pkg/janet_world_wasm.js';
//!
//! await init();
//!
//! const world = new JanetWorldClient('wss://nats01.internal.plantange.net/', 'default', 'my-client');
//!
//! world.onChunkActivated((chunkId, cx, cy, seed, lod, chunkSize) => {
//!   spawnTerrain(cx, cy, seed);
//! });
//! world.onEntityTransform((id, x, y, z, rotY, vx, vy, vz, frame, dt) => {
//!   moveEntity(id, x, y, z);
//! });
//!
//! await world.connect();
//!
//! // In your render loop:
//! function tick() {
//!   world.poll();
//!   requestAnimationFrame(tick);
//! }
//! tick();
//! ```

use wasm_bindgen::prelude::*;

use crate::bridge::{BridgeConfig, BridgeHandle};
use crate::cache::ClientWorldCache;
use crate::events::WorldEvent;
use crate::nats_ws;

// ---------------------------------------------------------------------------
// JanetWorldClient
// ---------------------------------------------------------------------------

/// Primary Wasm API object.
///
/// Instantiate with `new JanetWorldClient(endpoint, session, participantId)`.
/// Call `await world.connect()` once, then `world.poll()` each animation frame.
#[wasm_bindgen]
pub struct JanetWorldClient {
    config: BridgeConfig,
    bridge: Option<BridgeHandle>,
    cache: ClientWorldCache,

    // JS callback storage (Option<js_sys::Function>)
    on_connection_state: Option<js_sys::Function>,
    on_chunk_activated: Option<js_sys::Function>,
    on_chunk_deactivated: Option<js_sys::Function>,
    on_structure_spawned: Option<js_sys::Function>,
    on_structure_removed: Option<js_sys::Function>,
    on_entity_spawned: Option<js_sys::Function>,
    on_entity_removed: Option<js_sys::Function>,
    on_entity_transform: Option<js_sys::Function>,
    on_snapshot_begin: Option<js_sys::Function>,
    on_snapshot_end: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl JanetWorldClient {
    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// Create a new client.
    ///
    /// @param endpoint  - WebSocket URL of the NATS server (e.g. `wss://nats01.internal.plantange.net/`)
    /// @param session   - Janet session name (e.g. `"default"`)
    /// @param participantId - Identity to advertise on the bus
    #[wasm_bindgen(constructor)]
    pub fn new(endpoint: &str, session: &str, participant_id: &str) -> Self {
        Self {
            config: BridgeConfig {
                endpoint: endpoint.into(),
                session: session.into(),
                participant_id: participant_id.into(),
                ..Default::default()
            },
            bridge: None,
            cache: ClientWorldCache::new(),

            on_connection_state: None,
            on_chunk_activated: None,
            on_chunk_deactivated: None,
            on_structure_spawned: None,
            on_structure_removed: None,
            on_entity_spawned: None,
            on_entity_removed: None,
            on_entity_transform: None,
            on_snapshot_begin: None,
            on_snapshot_end: None,
        }
    }

    // -----------------------------------------------------------------------
    // Connection
    // -----------------------------------------------------------------------

    /// Open the WebSocket connection and start the bridge loop.
    ///
    /// Safe to call again after disconnecting.
    #[wasm_bindgen]
    pub fn connect(&mut self) -> Result<(), JsValue> {
        if self.bridge.is_some() {
            return Ok(()); // already connected
        }

        self.fire_connection_state("connecting");

        let handle = crate::bridge::spawn(self.config.clone())?;
        self.bridge = Some(handle);
        Ok(())
    }

    /// Close the WebSocket and clear local state.
    #[wasm_bindgen]
    pub fn disconnect(&mut self) {
        self.bridge = None;
        self.cache.clear();
        self.fire_connection_state("disconnected");
    }

    // -----------------------------------------------------------------------
    // poll() — must be called each animation frame
    // -----------------------------------------------------------------------

    /// Drain the event queue and fire registered callbacks.
    ///
    /// Call this once per `requestAnimationFrame` tick.  It is synchronous
    /// and cheap — typically < 1 ms even with hundreds of events.
    #[wasm_bindgen]
    pub fn poll(&mut self) {
        // Collect everything we need from the immutable borrow before any
        // mutable operations (apply_event, bridge = None) happen.
        let (events, is_alive) = match &self.bridge {
            Some(b) => (b.drain_events(256), b.is_alive()),
            None => return,
        };

        for event in events {
            self.apply_event(event);
        }

        // Do NOT null out self.bridge here even if !is_alive.
        // The bridge async loop may not have had a chance to push
        // WorldEvent::Disconnected yet (it sleeps 8ms before checking).
        // Dropping the handle here would orphan that event and leave the
        // JS state machine stuck in "connecting" forever.
        // Cleanup happens exclusively inside apply_event(Disconnected).
        let _ = is_alive; // suppress unused warning
    }

    // -----------------------------------------------------------------------
    // Callback registration
    // -----------------------------------------------------------------------

    /// `callback(state: string)` — `"connecting"` | `"active"` | `"disconnected"` | `"error"`
    #[wasm_bindgen(js_name = onConnectionState)]
    pub fn on_connection_state(&mut self, cb: js_sys::Function) {
        self.on_connection_state = Some(cb);
    }

    /// `callback(chunkId: string, cx: number, cy: number, seed: bigint, lod: number, chunkSize: number)`
    #[wasm_bindgen(js_name = onChunkActivated)]
    pub fn on_chunk_activated(&mut self, cb: js_sys::Function) {
        self.on_chunk_activated = Some(cb);
    }

    /// `callback(chunkId: string)`
    #[wasm_bindgen(js_name = onChunkDeactivated)]
    pub fn on_chunk_deactivated(&mut self, cb: js_sys::Function) {
        self.on_chunk_deactivated = Some(cb);
    }

    /// `callback(structureId: string, typeId: string, x: number, y: number, z: number, rotY: number)`
    #[wasm_bindgen(js_name = onStructureSpawned)]
    pub fn on_structure_spawned(&mut self, cb: js_sys::Function) {
        self.on_structure_spawned = Some(cb);
    }

    /// `callback(structureId: string)`
    #[wasm_bindgen(js_name = onStructureRemoved)]
    pub fn on_structure_removed(&mut self, cb: js_sys::Function) {
        self.on_structure_removed = Some(cb);
    }

    /// `callback(entityId: string, archetype: string, x: number, y: number, z: number, rotY: number)`
    #[wasm_bindgen(js_name = onEntitySpawned)]
    pub fn on_entity_spawned(&mut self, cb: js_sys::Function) {
        self.on_entity_spawned = Some(cb);
    }

    /// `callback(entityId: string)`
    #[wasm_bindgen(js_name = onEntityRemoved)]
    pub fn on_entity_removed(&mut self, cb: js_sys::Function) {
        self.on_entity_removed = Some(cb);
    }

    /// `callback(entityId, x, y, z, rotY, vx, vy, vz, frame: bigint, dt: number)`
    #[wasm_bindgen(js_name = onEntityTransform)]
    pub fn on_entity_transform(&mut self, cb: js_sys::Function) {
        self.on_entity_transform = Some(cb);
    }

    /// `callback(frame: bigint)`
    #[wasm_bindgen(js_name = onSnapshotBegin)]
    pub fn on_snapshot_begin(&mut self, cb: js_sys::Function) {
        self.on_snapshot_begin = Some(cb);
    }

    /// `callback()`
    #[wasm_bindgen(js_name = onSnapshotEnd)]
    pub fn on_snapshot_end(&mut self, cb: js_sys::Function) {
        self.on_snapshot_end = Some(cb);
    }

    // -----------------------------------------------------------------------
    // Intent methods
    // -----------------------------------------------------------------------

    /// Send a movement intent (`dx/dy/dz` should be a unit or sub-unit vector).
    #[wasm_bindgen(js_name = sendMovement)]
    pub fn send_movement(&self, dx: f32, dy: f32, dz: f32) {
        let payload = serde_json::json!({
            "id": self.config.participant_id, "dx": dx, "dy": dy, "dz": dz
        });
        self.publish_intent("intent.move", &payload.to_string());
    }

    /// Send an interaction intent (optionally with a verb like `"open"`, `"attack"`).
    #[wasm_bindgen(js_name = sendInteraction)]
    pub fn send_interaction(&self, target_id: &str, verb: Option<String>) {
        let payload = serde_json::json!({
            "id": self.config.participant_id,
            "target_id": target_id,
            "verb": verb,
        });
        self.publish_intent("intent.interact", &payload.to_string());
    }

    /// Request a server-authorised teleport.
    #[wasm_bindgen]
    pub fn teleport(&self, x: f32, y: f32, z: f32) {
        let payload = serde_json::json!({
            "id": self.config.participant_id, "x": x, "y": y, "z": z
        });
        self.publish_intent("intent.teleport", &payload.to_string());
    }

    /// Advise the server of the client's view radius.
    #[wasm_bindgen(js_name = updateViewRadius)]
    pub fn update_view_radius(&self, radius: f32) {
        let payload = serde_json::json!({
            "id": self.config.participant_id, "radius": radius
        });
        self.publish_intent("intent.view_radius", &payload.to_string());
    }

    /// Request a full world snapshot for the given position and radius.
    ///
    /// Useful on reconnect or after a scene transition.
    #[wasm_bindgen(js_name = requestSnapshot)]
    pub fn request_snapshot(&self, x: f32, y: f32, z: f32, radius: f32) {
        let payload = serde_json::json!({
            "id": self.config.participant_id, "x": x, "y": y, "z": z, "radius": radius
        });
        self.publish_intent("world.cmd.snapshot", &payload.to_string());
    }

    // -----------------------------------------------------------------------
    // Cache queries
    // -----------------------------------------------------------------------

    /// Number of currently active terrain chunks.
    #[wasm_bindgen(js_name = activeChunkCount)]
    pub fn active_chunk_count(&self) -> u32 {
        self.cache.chunk_count() as u32
    }

    /// Number of tracked entities.
    #[wasm_bindgen(js_name = entityCount)]
    pub fn entity_count(&self) -> u32 {
        self.cache.entity_count() as u32
    }

    /// Number of active structures.
    #[wasm_bindgen(js_name = structureCount)]
    pub fn structure_count(&self) -> u32 {
        self.cache.structure_count() as u32
    }

    /// True if the given chunk ID is currently active.
    #[wasm_bindgen(js_name = isChunkActive)]
    pub fn is_chunk_active(&self, chunk_id: &str) -> bool {
        self.cache.is_chunk_active(chunk_id)
    }

    /// True if the bridge is alive.
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.bridge.as_ref().map_or(false, |b| b.is_alive())
    }

    /// Server frame of the most recent event processed.
    #[wasm_bindgen(js_name = lastFrame)]
    pub fn last_frame(&self) -> u32 {
        self.cache.last_frame as u32
    }

    /// Dead-reckon an entity `elapsedSec` seconds past its last known state.
    ///
    /// Returns `[x, y, z]` or `null` if the entity is not cached.
    #[wasm_bindgen(js_name = extrapolateEntity)]
    pub fn extrapolate_entity(&self, entity_id: &str, elapsed: f32) -> Option<Vec<f32>> {
        self.cache.entities.get(entity_id).map(|e| {
            let (x, y, z) = e.extrapolated(elapsed);
            vec![x, y, z]
        })
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

impl JanetWorldClient {
    /// Apply a single WorldEvent to the cache and fire the registered JS callback.
    fn apply_event(&mut self, event: WorldEvent) {
        match event {
            WorldEvent::Connected {
                session: _,
                participant_id: _,
                frame: _,
            } => {
                self.fire_connection_state("active");
            }

            WorldEvent::Disconnected { reason } => {
                log::warn!("[client] Disconnected: {}", reason);
                self.bridge = None;
                self.cache.clear();
                self.fire_connection_state("disconnected");
            }

            WorldEvent::ChunkActivated {
                chunk_id,
                cx,
                cy,
                seed,
                lod,
                chunk_size,
            } => {
                self.cache
                    .activate_chunk(chunk_id.clone(), cx, cy, seed, lod, chunk_size);
                if !self.cache.in_snapshot {
                    call_fn(
                        &self.on_chunk_activated,
                        &[
                            JsValue::from_str(&chunk_id),
                            JsValue::from(cx),
                            JsValue::from(cy),
                            // seed as f64 (JS BigInt would be ideal but f64 is fine for u64 seeds)
                            JsValue::from(seed as f64),
                            JsValue::from(lod as u32),
                            JsValue::from(chunk_size),
                        ],
                    );
                }
            }

            WorldEvent::ChunkDeactivated { chunk_id } => {
                self.cache.deactivate_chunk(&chunk_id);
                if !self.cache.in_snapshot {
                    call_fn(&self.on_chunk_deactivated, &[JsValue::from_str(&chunk_id)]);
                }
            }

            WorldEvent::StructureSpawned {
                structure_id,
                type_id,
                x,
                y,
                z,
                rotation_y,
            } => {
                self.cache.spawn_structure(
                    structure_id.clone(),
                    type_id.clone(),
                    x,
                    y,
                    z,
                    rotation_y,
                );
                if !self.cache.in_snapshot {
                    call_fn(
                        &self.on_structure_spawned,
                        &[
                            JsValue::from_str(&structure_id),
                            JsValue::from_str(&type_id),
                            JsValue::from(x),
                            JsValue::from(y),
                            JsValue::from(z),
                            JsValue::from(rotation_y),
                        ],
                    );
                }
            }

            WorldEvent::StructureRemoved { structure_id } => {
                self.cache.remove_structure(&structure_id);
                if !self.cache.in_snapshot {
                    call_fn(
                        &self.on_structure_removed,
                        &[JsValue::from_str(&structure_id)],
                    );
                }
            }

            WorldEvent::EntitySpawned {
                entity_id,
                archetype,
                x,
                y,
                z,
                rotation_y,
            } => {
                self.cache
                    .spawn_entity(entity_id.clone(), archetype.clone(), x, y, z, rotation_y);
                if !self.cache.in_snapshot {
                    call_fn(
                        &self.on_entity_spawned,
                        &[
                            JsValue::from_str(&entity_id),
                            JsValue::from_str(&archetype),
                            JsValue::from(x),
                            JsValue::from(y),
                            JsValue::from(z),
                            JsValue::from(rotation_y),
                        ],
                    );
                }
            }

            WorldEvent::EntityRemoved { entity_id } => {
                self.cache.remove_entity(&entity_id);
                if !self.cache.in_snapshot {
                    call_fn(&self.on_entity_removed, &[JsValue::from_str(&entity_id)]);
                }
            }

            WorldEvent::EntityTransform {
                entity_id,
                x,
                y,
                z,
                rotation_y,
                vx,
                vy,
                vz,
                frame,
                dt,
            } => {
                self.cache.update_entity_transform(
                    &entity_id, x, y, z, rotation_y, vx, vy, vz, frame, dt,
                );
                self.cache.last_frame = self.cache.last_frame.max(frame);
                call_fn(
                    &self.on_entity_transform,
                    &[
                        JsValue::from_str(&entity_id),
                        JsValue::from(x),
                        JsValue::from(y),
                        JsValue::from(z),
                        JsValue::from(rotation_y),
                        JsValue::from(vx),
                        JsValue::from(vy),
                        JsValue::from(vz),
                        JsValue::from(frame as f64),
                        JsValue::from(dt),
                    ],
                );
            }

            WorldEvent::SnapshotBegin { frame } => {
                self.cache.in_snapshot = true;
                self.cache.clear();
                call_fn(&self.on_snapshot_begin, &[JsValue::from(frame as f64)]);
            }

            WorldEvent::SnapshotEnd => {
                self.cache.in_snapshot = false;
                call_fn(&self.on_snapshot_end, &[]);
            }
        }
    }

    fn fire_connection_state(&self, state: &str) {
        call_fn(&self.on_connection_state, &[JsValue::from_str(state)]);
    }

    fn publish_intent(&self, subject: &str, json_payload: &str) {
        if let Some(bridge) = &self.bridge {
            let frame = nats_ws::pub_frame(subject, json_payload);
            bridge.queue_intent(frame);
        }
    }
}

// ---------------------------------------------------------------------------
// JS callback helper
// ---------------------------------------------------------------------------

fn call_fn(f: &Option<js_sys::Function>, args: &[JsValue]) {
    if let Some(func) = f {
        let this = JsValue::NULL;
        let arr = js_sys::Array::new();
        for a in args {
            arr.push(a);
        }
        if let Err(e) = func.apply(&this, &arr) {
            log::warn!("[client] Callback error: {:?}", e);
        }
    }
}
