//! Semantic world events delivered from the NATS bridge to the JS layer.
//!
//! This module is a near-direct port of the Godot client's `events.rs`.
//! WASM is single-threaded so there is no `Send` / `Sync` requirement,
//! but we derive them anyway so the module is also usable in native tests.
//!
//! All coordinates are world-space metres.  All angles are radians.

/// A single semantic world event.
///
/// The bridge thread translates raw NATS messages into these types and
/// queues them for the main JS call frame via `JanetWorldClient::poll()`.
#[derive(Debug, Clone)]
pub enum WorldEvent {
    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------
    /// NATS connection established and `CONNECT` handshake complete.
    Connected {
        session: String,
        participant_id: String,
        frame: u64,
    },
    /// Connection closed or lost.  `reason` is human-readable.
    Disconnected {
        reason: String,
    },

    // ------------------------------------------------------------------
    // Terrain / chunks
    // ------------------------------------------------------------------
    /// Server activated a terrain chunk.
    ///
    /// The client generates the mesh locally from `seed`.
    /// No heightmap is ever sent over the wire.
    ChunkActivated {
        chunk_id: String,
        cx: i32,
        cy: i32,
        seed: u64,
        lod: u8,
        chunk_size: f32,
    },

    /// Server deactivated a terrain chunk — safe to free resources.
    ChunkDeactivated {
        chunk_id: String,
    },

    // ------------------------------------------------------------------
    // Structures (static world objects)
    // ------------------------------------------------------------------
    StructureSpawned {
        structure_id: String,
        /// Asset / scene identifier (game-defined).
        type_id: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    },
    StructureRemoved {
        structure_id: String,
    },

    // ------------------------------------------------------------------
    // Entities (dynamic actors)
    // ------------------------------------------------------------------
    EntitySpawned {
        entity_id: String,
        /// Game-defined archetype string (e.g. "creature/wolf").
        archetype: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    },
    EntityRemoved {
        entity_id: String,
    },

    /// Authoritative server transform.  Sent at tick rate (~10–30 Hz).
    ///
    /// Velocity (`vx/vy/vz`) enables dead-reckoning between frames.
    EntityTransform {
        entity_id: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        /// Server frame this transform was produced on.
        frame: u64,
        /// Physics integration step (seconds).
        dt: f32,
    },

    // ------------------------------------------------------------------
    // Snapshot (full state on connect / reconnect)
    // ------------------------------------------------------------------
    /// Marks the start of a full-state snapshot.
    ///
    /// Consumers should suppress per-entity UI updates until `SnapshotEnd`.
    SnapshotBegin {
        frame: u64,
    },

    /// Snapshot fully delivered — resume normal event processing.
    SnapshotEnd,
}
