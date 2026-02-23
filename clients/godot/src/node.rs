//! `JanetWorldClient` – the primary Godot 4 node.
//!
//! ## Drop-in usage
//!
//! 1. Enable the `janet_world` plugin in Project → Plugins.
//! 2. Add a `JanetWorldClient` node to your scene.
//! 3. Set the exported properties (or leave defaults for local dev).
//! 4. Connect the signals you care about in GDScript / Inspector.
//! 5. Press Play – the world streams automatically.
//!
//! ## GDScript example
//!
//! ```gdscript
//! @onready var world := $JanetWorldClient
//!
//! func _ready():
//!     world.chunk_activated.connect(_on_chunk_activated)
//!     world.entity_transform.connect(_on_entity_transform)
//!
//! func _on_chunk_activated(chunk_id, cx, cy, seed, lod, chunk_size):
//!     spawn_terrain_mesh(cx, cy, seed, lod, chunk_size)
//!
//! func _on_entity_transform(entity_id, x, y, z, rot_y, vx, vy, vz, frame, dt):
//!     move_entity_proxy(entity_id, Vector3(x, y, z))
//! ```

use godot::prelude::*;

use crate::bridge::{BridgeConfig, BusHandle, IntentMessage};
use crate::cache::ClientWorldCache;
use crate::events::WorldEvent;

// ---------------------------------------------------------------------------
// Node definition
// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node)]
pub struct JanetWorldClient {
    // ---- exported editor properties ----------------------------------------

    /// NATS endpoint for the janet bus (e.g. "nats://localhost:4222").
    #[export]
    pub endpoint: GString,

    /// Janet session name.
    #[export]
    pub session: GString,

    /// Participant ID this client advertises on the bus.
    #[export]
    pub participant_id: GString,

    /// Connect automatically when the node enters the scene tree.
    #[export]
    pub auto_connect: bool,

    // ---- runtime state (not exported) ---------------------------------------

    /// Bridge handle — present only while connected.
    handle: Option<BusHandle>,

    /// Local world state mirror, updated from bus events each frame.
    cache: ClientWorldCache,

    base: Base<Node>,
}

// ---------------------------------------------------------------------------
// Godot lifecycle
// ---------------------------------------------------------------------------

#[godot_api]
impl INode for JanetWorldClient {
    fn init(base: Base<Node>) -> Self {
        Self {
            endpoint: "nats://localhost:4222".into(),
            session: "default".into(),
            participant_id: "godot-client".into(),
            auto_connect: true,
            handle: None,
            cache: ClientWorldCache::new(),
            base,
        }
    }

    fn ready(&mut self) {
        if self.auto_connect {
            self.connect_to_world();
        }
    }

    /// Called each rendered frame.  Drains the event queue from the bridge
    /// thread, applies events to the cache, and emits signals.
    fn process(&mut self, _delta: f64) {
        let events: Vec<WorldEvent> = match &self.handle {
            Some(h) => h.poll(),
            None => return,
        };

        for event in events {
            self.apply_event(event);
        }
    }
}

// ---------------------------------------------------------------------------
// Signals and exported methods
// ---------------------------------------------------------------------------

#[godot_api]
impl JanetWorldClient {
    // ====================================================================
    // Signals
    // ====================================================================

    /// Emitted when the client's bus connection state changes.
    /// `state` is one of: "connecting", "active", "disconnected", "error".
    #[signal]
    fn connection_state_changed(state: GString);

    /// A terrain chunk became active.  Use `seed` + `(cx, cy)` to generate
    /// the mesh deterministically — no height data is sent over the wire.
    #[signal]
    fn chunk_activated(
        chunk_id: GString,
        cx: i32,
        cy: i32,
        seed: i64,
        lod: i32,
        chunk_size: f32,
    );

    /// A terrain chunk was deactivated and should be freed.
    #[signal]
    fn chunk_deactivated(chunk_id: GString);

    /// A static structure entered the active region.
    /// `type_id` is the asset/scene path your project uses to instantiate it.
    #[signal]
    fn structure_spawned(
        structure_id: GString,
        type_id: GString,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    );

    /// A static structure was removed.
    #[signal]
    fn structure_removed(structure_id: GString);

    /// A dynamic entity (creature, vehicle, …) entered the active region.
    #[signal]
    fn entity_spawned(
        entity_id: GString,
        archetype: GString,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    );

    /// A dynamic entity left the active region.
    #[signal]
    fn entity_removed(entity_id: GString);

    /// Authoritative transform update for a live entity.
    ///
    /// Velocity components (`vx, vy, vz`) allow dead-reckoning in GDScript
    /// between server updates.
    #[signal]
    fn entity_transform(
        entity_id: GString,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        frame: i64,
        dt: f32,
    );

    /// Emitted once before a snapshot floods the client with events.
    /// Use this to suppress UI flicker or batch spawning.
    #[signal]
    fn snapshot_begin(frame: i64);

    /// Emitted when the snapshot has been fully applied.
    #[signal]
    fn snapshot_end();

    // ====================================================================
    // Connection management
    // ====================================================================

    /// Connect to the world service on the janet bus.
    ///
    /// Called automatically if `auto_connect = true` (default).
    /// Safe to call again after disconnecting.
    #[func]
    pub fn connect_to_world(&mut self) {
        if self.handle.is_some() {
            godot_warn!("JanetWorldClient: already connected — ignoring connect_to_world()");
            return;
        }

        let config = BridgeConfig {
            endpoint: self.endpoint.to_string(),
            session: self.session.to_string(),
            participant_id: self.participant_id.to_string(),
            ..Default::default()
        };

        godot_print!(
            "JanetWorldClient: connecting to {} (session={})",
            config.endpoint,
            config.session
        );

        self.base_mut().emit_signal(
            "connection_state_changed",
            &["connecting".to_variant()],
        );

        self.handle = Some(crate::bridge::spawn(config));
    }

    /// Gracefully disconnect from the world service.
    #[func]
    pub fn disconnect_from_world(&mut self) {
        if let Some(ref h) = self.handle {
            h.send_intent(IntentMessage::Disconnect);
        }
        self.handle = None;
        self.cache.clear();

        self.base_mut().emit_signal(
            "connection_state_changed",
            &["disconnected".to_variant()],
        );
    }

    // ====================================================================
    // Intent methods — Godot developer API
    // ====================================================================

    /// Send a movement intent.  `direction` should be a unit or sub-unit vector.
    ///
    /// The server resolves movement authority; this is a *hint*, not a
    /// position override.
    #[func]
    pub fn send_movement(&self, direction: Vector3) {
        self.send_intent(IntentMessage::Move {
            dx: direction.x,
            dy: direction.y,
            dz: direction.z,
        });
    }

    /// Send an interaction intent (open door, talk to NPC, attack, etc.).
    #[func]
    pub fn send_interaction(&self, target_id: GString) {
        self.send_intent(IntentMessage::Interact {
            target_id: target_id.to_string(),
            verb: None,
        });
    }

    /// Send an interaction intent with an explicit verb.
    #[func]
    pub fn send_interaction_verb(&self, target_id: GString, verb: GString) {
        self.send_intent(IntentMessage::Interact {
            target_id: target_id.to_string(),
            verb: Some(verb.to_string()),
        });
    }

    /// Request a server-authorised teleport.
    #[func]
    pub fn teleport(&self, position: Vector3) {
        self.send_intent(IntentMessage::Teleport {
            x: position.x,
            y: position.y,
            z: position.z,
        });
    }

    /// Advise the server of the client's view radius so it can tune
    /// streaming density.
    #[func]
    pub fn update_view_radius(&self, radius: f32) {
        self.send_intent(IntentMessage::ViewRadius { radius });
    }

    /// Request a full world snapshot for the given position + radius.
    ///
    /// Useful on reconnect or scene transition.
    #[func]
    pub fn request_snapshot(&self, position: Vector3, radius: f32) {
        self.send_intent(IntentMessage::RequestSnapshot {
            x: position.x,
            y: position.y,
            z: position.z,
            radius,
        });
    }

    // ====================================================================
    // Cache queries — read-only access to local world state
    // ====================================================================

    /// Returns the number of currently active terrain chunks.
    #[func]
    pub fn active_chunk_count(&self) -> i32 {
        self.cache.chunk_count() as i32
    }

    /// Returns the number of currently tracked entities.
    #[func]
    pub fn entity_count(&self) -> i32 {
        self.cache.entity_count() as i32
    }

    /// Returns the number of currently active structures.
    #[func]
    pub fn structure_count(&self) -> i32 {
        self.cache.structure_count() as i32
    }

    /// Returns `true` if the given chunk ID is currently active.
    #[func]
    pub fn is_chunk_active(&self, chunk_id: GString) -> bool {
        self.cache.is_chunk_active(&chunk_id.to_string())
    }

    /// Returns `true` if connected to the bus.
    #[func]
    pub fn is_connected_to_world(&self) -> bool {
        self.handle.is_some()
    }

    /// Returns the last server frame number received.
    #[func]
    pub fn last_frame(&self) -> i64 {
        self.cache.last_frame as i64
    }

    /// Dead-reckon an entity's current position by `elapsed` seconds since
    /// the last authoritative update.  Returns `Vector3.ZERO` if unknown.
    #[func]
    pub fn extrapolate_entity(&self, entity_id: GString, elapsed: f32) -> Vector3 {
        match self.cache.entities.get(&entity_id.to_string()) {
            Some(e) => {
                let (x, y, z) = e.extrapolated(elapsed);
                Vector3::new(x, y, z)
            }
            None => Vector3::ZERO,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

impl JanetWorldClient {
    fn send_intent(&self, intent: IntentMessage) {
        if let Some(ref h) = self.handle {
            h.send_intent(intent);
        }
    }

    /// Apply a single `WorldEvent` to the cache and emit the corresponding
    /// Godot signal.  Runs on the main thread inside `_process`.
    fn apply_event(&mut self, event: WorldEvent) {
        match event {
            WorldEvent::Connected {
                session: _,
                participant_id: _,
                frame: _,
            } => {
                self.base_mut().emit_signal(
                    "connection_state_changed",
                    &["active".to_variant()],
                );
            }

            WorldEvent::Disconnected { reason } => {
                godot_warn!("JanetWorldClient: disconnected – {}", reason);
                self.handle = None;
                self.cache.clear();
                self.base_mut().emit_signal(
                    "connection_state_changed",
                    &["disconnected".to_variant()],
                );
            }

            WorldEvent::ChunkActivated {
                chunk_id,
                cx,
                cy,
                seed,
                lod,
                chunk_size,
            } => {
                self.cache.activate_chunk(
                    chunk_id.clone(),
                    cx,
                    cy,
                    seed,
                    lod,
                    chunk_size,
                );
                if !self.cache.in_snapshot {
                    self.base_mut().emit_signal(
                        "chunk_activated",
                        &[
                            GString::from(chunk_id.as_str()).to_variant(),
                            cx.to_variant(),
                            cy.to_variant(),
                            (seed as i64).to_variant(),
                            (lod as i32).to_variant(),
                            chunk_size.to_variant(),
                        ],
                    );
                }
            }

            WorldEvent::ChunkDeactivated { chunk_id } => {
                self.cache.deactivate_chunk(&chunk_id);
                if !self.cache.in_snapshot {
                    self.base_mut().emit_signal(
                        "chunk_deactivated",
                        &[GString::from(chunk_id.as_str()).to_variant()],
                    );
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
                    self.base_mut().emit_signal(
                        "structure_spawned",
                        &[
                            GString::from(structure_id.as_str()).to_variant(),
                            GString::from(type_id.as_str()).to_variant(),
                            x.to_variant(),
                            y.to_variant(),
                            z.to_variant(),
                            rotation_y.to_variant(),
                        ],
                    );
                }
            }

            WorldEvent::StructureRemoved { structure_id } => {
                self.cache.remove_structure(&structure_id);
                if !self.cache.in_snapshot {
                    self.base_mut().emit_signal(
                        "structure_removed",
                        &[GString::from(structure_id.as_str()).to_variant()],
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
                    self.base_mut().emit_signal(
                        "entity_spawned",
                        &[
                            GString::from(entity_id.as_str()).to_variant(),
                            GString::from(archetype.as_str()).to_variant(),
                            x.to_variant(),
                            y.to_variant(),
                            z.to_variant(),
                            rotation_y.to_variant(),
                        ],
                    );
                }
            }

            WorldEvent::EntityRemoved { entity_id } => {
                self.cache.remove_entity(&entity_id);
                if !self.cache.in_snapshot {
                    self.base_mut().emit_signal(
                        "entity_removed",
                        &[GString::from(entity_id.as_str()).to_variant()],
                    );
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
                self.base_mut().emit_signal(
                    "entity_transform",
                    &[
                        GString::from(entity_id.as_str()).to_variant(),
                        x.to_variant(),
                        y.to_variant(),
                        z.to_variant(),
                        rotation_y.to_variant(),
                        vx.to_variant(),
                        vy.to_variant(),
                        vz.to_variant(),
                        (frame as i64).to_variant(),
                        dt.to_variant(),
                    ],
                );
            }

            WorldEvent::SnapshotBegin { frame } => {
                self.cache.in_snapshot = true;
                self.cache.clear();
                self.base_mut()
                    .emit_signal("snapshot_begin", &[(frame as i64).to_variant()]);
            }

            WorldEvent::SnapshotEnd => {
                self.cache.in_snapshot = false;
                // Now that the snapshot is fully applied, flush all pending
                // entity/chunk/structure events as a batch via snapshot_end.
                self.base_mut().emit_signal("snapshot_end", &[]);

                // Emit individual spawns so GDScript can instantiate nodes.
                for chunk in self.cache.chunks.values().cloned().collect::<Vec<_>>() {
                    self.base_mut().emit_signal(
                        "chunk_activated",
                        &[
                            GString::from(chunk.chunk_id.as_str()).to_variant(),
                            chunk.cx.to_variant(),
                            chunk.cy.to_variant(),
                            (chunk.seed as i64).to_variant(),
                            (chunk.lod as i32).to_variant(),
                            chunk.chunk_size.to_variant(),
                        ],
                    );
                }
                for structure in self.cache.structures.values().cloned().collect::<Vec<_>>() {
                    self.base_mut().emit_signal(
                        "structure_spawned",
                        &[
                            GString::from(structure.structure_id.as_str()).to_variant(),
                            GString::from(structure.type_id.as_str()).to_variant(),
                            structure.x.to_variant(),
                            structure.y.to_variant(),
                            structure.z.to_variant(),
                            structure.rotation_y.to_variant(),
                        ],
                    );
                }
                for entity in self.cache.entities.values().cloned().collect::<Vec<_>>() {
                    self.base_mut().emit_signal(
                        "entity_spawned",
                        &[
                            GString::from(entity.entity_id.as_str()).to_variant(),
                            GString::from(entity.archetype.as_str()).to_variant(),
                            entity.x.to_variant(),
                            entity.y.to_variant(),
                            entity.z.to_variant(),
                            entity.rotation_y.to_variant(),
                        ],
                    );
                }
            }
        }
    }
}
