//! `world.*` and `intent.*` wire protocol.
//!
//! This module owns **every message that crosses the bus boundary** between
//! the world service and any consumer (Godot, web browser, another server…).
//!
//! ## Channel namespaces
//!
//! | Namespace     | Direction          | Carried by          |
//! |---------------|--------------------|---------------------|
//! | `world.*`     | server → client    | NATS subject pub    |
//! | `intent.*`    | client → server    | janet command       |
//! | `world.cmd.*` | client → server    | janet command (req) |
//!
//! ## Design rules
//!
//! 1. Every struct must be `Serialize + Deserialize` with snake_case JSON.
//! 2. No physics-layer types leak out (`ColliderShape`, `BodyParams`, etc.).
//! 3. Terrain is **never** sent as raw height arrays — only `(cx, cy, seed, lod)`.
//! 4. Every outbound event includes `frame: u64` and `session: String`.
//! 5. Transforms include `dt: f32` to support client-side interpolation.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Common envelope
// ---------------------------------------------------------------------------

/// Every outbound message is wrapped in this envelope.
///
/// The `session` field lets multiplexed clients distinguish worlds.
/// The `frame` field lets clients timestamp-sort interleaved streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldEvent<T> {
    pub session: String,
    pub frame: u64,
    pub payload: T,
}

impl<T> WorldEvent<T> {
    pub fn new(session: impl Into<String>, frame: u64, payload: T) -> Self {
        Self {
            session: session.into(),
            frame,
            payload,
        }
    }
}

// ---------------------------------------------------------------------------
// Terrain / chunk events  (subjects: world.chunk.*)
// ---------------------------------------------------------------------------

/// Server instructs client to activate a chunk.
///
/// Client generates terrain locally using `seed` and the chunk coordinate —
/// raw height data is **never** sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkActivated {
    /// Globally unique chunk identifier (deterministic: `{session}:{cx}:{cy}`).
    pub chunk_id: String,
    pub cx: i32,
    pub cy: i32,
    /// Terrain seed — sufficient for deterministic local generation.
    pub seed: u64,
    /// 0 = full detail, 1 = half, 2 = quarter.
    pub lod: u8,
    /// World-space size of one chunk side.
    pub chunk_size: f32,
}

/// Server instructs client to free a chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkDeactivated {
    pub chunk_id: String,
}

// ---------------------------------------------------------------------------
// Structure events  (subjects: world.structure.*)
// ---------------------------------------------------------------------------

/// A static structure appeared in the world (building, rock, obstacle…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureSpawned {
    pub structure_id: String,
    /// Asset / scene path the client uses to instantiate.
    pub type_id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// A static structure was removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureRemoved {
    pub structure_id: String,
}

// ---------------------------------------------------------------------------
// Entity events  (subjects: world.entity.*)
// ---------------------------------------------------------------------------

/// An entity (creature, vehicle, projectile…) entered the active region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySpawned {
    pub entity_id: String,
    /// Game-defined archetype string (e.g. "creature/wolf", "vehicle/cart").
    pub archetype: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// An entity left the active region or was destroyed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRemoved {
    pub entity_id: String,
}

/// Authoritative transform update for a live entity.
///
/// Sent at simulation tick rate (typically 10–30 Hz).
/// Clients interpolate between received frames at their render rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTransform {
    pub entity_id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    /// Velocity (m/s) for dead-reckoning / extrapolation.
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    /// Integration step that produced this transform.
    pub dt: f32,
}

// ---------------------------------------------------------------------------
// Snapshot  (subject: world.snapshot)
// ---------------------------------------------------------------------------

/// Full world state snapshot sent on initial connect or after reconnect.
///
/// Clients should hydrate their `ClientWorldCache` from this before
/// processing incremental events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub active_chunks: Vec<ChunkActivated>,
    pub structures: Vec<StructureSpawned>,
    pub entities: Vec<EntitySpawned>,
}

// ---------------------------------------------------------------------------
// Connection / lifecycle  (subject: world.connection.*)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Handshaking,
    Active,
    Degraded,
    Disconnected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub session: String,
    pub participant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Server frame when this status was emitted.
    pub frame: u64,
}

// ---------------------------------------------------------------------------
// Intent messages  (client → server, via intent.* commands)
// ---------------------------------------------------------------------------

/// Client indicates desired movement direction (unit vector, server resolves).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMove {
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
}

/// Client requests interaction with a specific entity or structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentInteract {
    pub target_id: String,
    /// Optional interaction verb (e.g. "open", "attack", "talk").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verb: Option<String>,
}

/// Client requests a teleport (authorised by server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentTeleport {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Client advertises its view radius so the server can tune the activation
/// window and census resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentViewRadius {
    pub radius: f32,
}

// ---------------------------------------------------------------------------
// World command requests  (client → server, request-reply via world.cmd.*)
// ---------------------------------------------------------------------------

/// Request a stats snapshot (reply: WorldStats JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdStats {}

/// Request a full world snapshot for this client's current position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdRequestSnapshot {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub radius: f32,
}

// ---------------------------------------------------------------------------
// Subject helpers
// ---------------------------------------------------------------------------

/// All NATS/bus subjects used by the world protocol, as constants.
pub mod subjects {
    pub const CHUNK_ACTIVATED: &str = "world.chunk.activated";
    pub const CHUNK_DEACTIVATED: &str = "world.chunk.deactivated";

    pub const STRUCTURE_SPAWNED: &str = "world.structure.spawned";
    pub const STRUCTURE_REMOVED: &str = "world.structure.removed";

    pub const ENTITY_SPAWNED: &str = "world.entity.spawned";
    pub const ENTITY_REMOVED: &str = "world.entity.removed";
    pub const ENTITY_TRANSFORM: &str = "world.entity.transform";

    pub const SNAPSHOT: &str = "world.snapshot";
    pub const CONNECTION_STATUS: &str = "world.connection.status";

    pub const INTENT_MOVE: &str = "intent.move";
    pub const INTENT_INTERACT: &str = "intent.interact";
    pub const INTENT_TELEPORT: &str = "intent.teleport";
    pub const INTENT_VIEW_RADIUS: &str = "intent.view_radius";

    pub const CMD_STATS: &str = "world.cmd.stats";
    pub const CMD_SNAPSHOT: &str = "world.cmd.snapshot";

    /// Management commands sent by the coordinator → world service.
    /// (Not used directly by clients.)
    pub mod mgmt {
        pub const PARTICIPANT_JOIN: &str = "world.participant.join";
        pub const PARTICIPANT_LEAVE: &str = "world.participant.leave";
        pub const TELEPORT: &str = "world.command.teleport";
        pub const STATS: &str = "world.command.stats";
    }
}
