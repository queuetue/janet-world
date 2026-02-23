//! `ClientWorldCache` – local mirror of active world state.
//!
//! The cache is populated by the bridge thread and read by the Godot thread
//! via the `JanetWorldClient` node.  It answers questions like:
//! - "Is chunk (cx, cy) currently active?"
//! - "Where is entity X right now?"
//! - "What structures are loaded?"
//!
//! The cache is NOT thread-safe by itself — the `JanetWorldClient` node
//! accesses it on the Godot main thread only (after draining the event queue).

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Sub-records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CachedChunk {
    pub chunk_id: String,
    pub cx: i32,
    pub cy: i32,
    pub seed: u64,
    pub lod: u8,
    pub chunk_size: f32,
}

#[derive(Debug, Clone)]
pub struct CachedStructure {
    pub structure_id: String,
    pub type_id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
}

#[derive(Debug, Clone)]
pub struct CachedEntity {
    pub entity_id: String,
    pub archetype: String,
    /// Last authoritative position.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    /// Last received velocity (for dead-reckoning).
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    /// Server frame when this transform was stamped.
    pub frame: u64,
    /// Integration dt from that frame.
    pub dt: f32,
}

impl CachedEntity {
    /// Dead-reckon position by `elapsed` seconds since `frame` was received.
    ///
    /// This is a simple linear extrapolation.  Replace with spline or
    /// hermite interpolation for smoother motion (Phase 5).
    pub fn extrapolated(&self, elapsed: f32) -> (f32, f32, f32) {
        (
            self.x + self.vx * elapsed,
            self.y + self.vy * elapsed,
            self.z + self.vz * elapsed,
        )
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Local world state mirror, maintained on the Godot main thread.
#[derive(Debug, Default)]
pub struct ClientWorldCache {
    /// Key: chunk_id
    pub chunks: HashMap<String, CachedChunk>,
    /// Key: structure_id
    pub structures: HashMap<String, CachedStructure>,
    /// Key: entity_id
    pub entities: HashMap<String, CachedEntity>,
    /// Frame number of the most recent update applied.
    pub last_frame: u64,
    /// Set to true while processing a snapshot to suppress per-event signals.
    pub in_snapshot: bool,
}

impl ClientWorldCache {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Chunks
    // ------------------------------------------------------------------

    pub fn activate_chunk(
        &mut self,
        chunk_id: String,
        cx: i32,
        cy: i32,
        seed: u64,
        lod: u8,
        chunk_size: f32,
    ) {
        self.chunks.insert(
            chunk_id.clone(),
            CachedChunk {
                chunk_id,
                cx,
                cy,
                seed,
                lod,
                chunk_size,
            },
        );
    }

    pub fn deactivate_chunk(&mut self, chunk_id: &str) {
        self.chunks.remove(chunk_id);
    }

    pub fn is_chunk_active(&self, chunk_id: &str) -> bool {
        self.chunks.contains_key(chunk_id)
    }

    // ------------------------------------------------------------------
    // Structures
    // ------------------------------------------------------------------

    pub fn spawn_structure(
        &mut self,
        structure_id: String,
        type_id: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    ) {
        self.structures.insert(
            structure_id.clone(),
            CachedStructure {
                structure_id,
                type_id,
                x,
                y,
                z,
                rotation_y,
            },
        );
    }

    pub fn remove_structure(&mut self, structure_id: &str) {
        self.structures.remove(structure_id);
    }

    // ------------------------------------------------------------------
    // Entities
    // ------------------------------------------------------------------

    pub fn spawn_entity(
        &mut self,
        entity_id: String,
        archetype: String,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
    ) {
        self.entities.insert(
            entity_id.clone(),
            CachedEntity {
                entity_id,
                archetype,
                x,
                y,
                z,
                rotation_y,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
                frame: 0,
                dt: 0.0,
            },
        );
    }

    pub fn remove_entity(&mut self, entity_id: &str) {
        self.entities.remove(entity_id);
    }

    pub fn update_entity_transform(
        &mut self,
        entity_id: &str,
        x: f32,
        y: f32,
        z: f32,
        rotation_y: f32,
        vx: f32,
        vy: f32,
        vz: f32,
        frame: u64,
        dt: f32,
    ) {
        if let Some(e) = self.entities.get_mut(entity_id) {
            e.x = x;
            e.y = y;
            e.z = z;
            e.rotation_y = rotation_y;
            e.vx = vx;
            e.vy = vy;
            e.vz = vz;
            e.frame = frame;
            e.dt = dt;
        }
    }

    // ------------------------------------------------------------------
    // Stats
    // ------------------------------------------------------------------

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn structure_count(&self) -> usize {
        self.structures.len()
    }

    /// Clear all state (e.g. after disconnect).
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.structures.clear();
        self.entities.clear();
        self.last_frame = 0;
        self.in_snapshot = false;
    }
}
