//! Semantic events delivered from the bridge to the Godot thread.
//!
//! These are **processed** events — the bridge translates raw bus messages
//! into these types before queueing them for the Godot thread.  No serde or
//! NATS types appear here.
//!
//! All coordinate values are in world-space metres.

/// A single semantic world event, safe to pass across the thread boundary.
#[derive(Debug, Clone)]
pub enum WorldEvent {
    // ------------------------------------------------------------------
    // Connection lifecycle
    // ------------------------------------------------------------------
    Connected {
        session: String,
        participant_id: String,
        frame: u64,
    },
    Disconnected {
        reason: String,
    },

    // ------------------------------------------------------------------
    // Terrain / chunks
    // ------------------------------------------------------------------
    ChunkActivated {
        chunk_id: String,
        cx: i32,
        cy: i32,
        seed: u64,
        lod: u8,
        chunk_size: f32,
    },
    ChunkDeactivated {
        chunk_id: String,
    },

    // ------------------------------------------------------------------
    // Structures
    // ------------------------------------------------------------------
    StructureSpawned {
        structure_id: String,
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
    // Entities
    // ------------------------------------------------------------------
    EntitySpawned {
        entity_id: String,
        archetype: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    },
    EntityRemoved {
        entity_id: String,
    },
    EntityTransform {
        entity_id: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        frame: u64,
        dt: f32,
    },

    // ------------------------------------------------------------------
    // Snapshot (initial hydration)
    // ------------------------------------------------------------------
    SnapshotBegin {
        frame: u64,
    },
    SnapshotEnd,
}
