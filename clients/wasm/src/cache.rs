//! `ClientWorldCache` — local mirror of active world state.
//!
//! Ported from the Godot client.  In the WASM client this struct lives
//! inside `JanetWorldClient` and is only ever accessed from the JS main
//! thread (no concurrent access, so no locking needed).

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
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    /// Last received velocity — used for dead-reckoning between ticks.
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    /// Server frame when this transform was stamped.
    pub frame: u64,
    pub dt: f32,
}

impl CachedEntity {
    /// Dead-reckon position `elapsed` seconds beyond the last known state.
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

/// Local world state mirror.  Updated by `JanetWorldClient::poll()` on the
/// JS main thread from the drained event queue.
#[derive(Debug, Default)]
pub struct ClientWorldCache {
    /// Active terrain chunks, keyed by `chunk_id`.
    pub chunks: HashMap<String, CachedChunk>,
    /// Active static structures, keyed by `structure_id`.
    pub structures: HashMap<String, CachedStructure>,
    /// Active dynamic entities, keyed by `entity_id`.
    pub entities: HashMap<String, CachedEntity>,
    /// Frame number from the most recent event processed.
    pub last_frame: u64,
    /// True while processing a snapshot — signals may be suppressed.
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

    #[allow(clippy::too_many_arguments)]
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
    // Counts
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

    /// Reset all state (called on disconnect).
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.structures.clear();
        self.entities.clear();
        self.last_frame = 0;
        self.in_snapshot = false;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Chunk lifecycle
    // ---------------------------------------------------------------

    #[test]
    fn activate_and_deactivate_chunk() {
        let mut c = ClientWorldCache::new();
        assert_eq!(c.chunk_count(), 0);

        c.activate_chunk("0:0".into(), 0, 0, 42, 0, 10.0);
        assert_eq!(c.chunk_count(), 1);
        assert!(c.is_chunk_active("0:0"));

        c.deactivate_chunk("0:0");
        assert_eq!(c.chunk_count(), 0);
        assert!(!c.is_chunk_active("0:0"));
    }

    #[test]
    fn activate_same_chunk_twice_replaces() {
        let mut c = ClientWorldCache::new();
        c.activate_chunk("0:0".into(), 0, 0, 42, 0, 10.0);
        c.activate_chunk("0:0".into(), 0, 0, 99, 1, 20.0);
        assert_eq!(c.chunk_count(), 1);
        assert_eq!(c.chunks["0:0"].seed, 99);
        assert_eq!(c.chunks["0:0"].lod, 1);
    }

    #[test]
    fn deactivate_nonexistent_chunk_is_noop() {
        let mut c = ClientWorldCache::new();
        c.deactivate_chunk("does-not-exist");
        assert_eq!(c.chunk_count(), 0);
    }

    // ---------------------------------------------------------------
    // Structure lifecycle
    // ---------------------------------------------------------------

    #[test]
    fn spawn_and_remove_structure() {
        let mut c = ClientWorldCache::new();
        c.spawn_structure("s1".into(), "tree".into(), 1.0, 0.0, 2.0, 0.0);
        assert_eq!(c.structure_count(), 1);
        assert_eq!(c.structures["s1"].type_id, "tree");

        c.remove_structure("s1");
        assert_eq!(c.structure_count(), 0);
    }

    // ---------------------------------------------------------------
    // Entity lifecycle
    // ---------------------------------------------------------------

    #[test]
    fn spawn_and_remove_entity() {
        let mut c = ClientWorldCache::new();
        c.spawn_entity("e1".into(), "creature/wolf".into(), 5.0, 0.0, 10.0, 1.57);
        assert_eq!(c.entity_count(), 1);
        assert_eq!(c.entities["e1"].archetype, "creature/wolf");

        c.remove_entity("e1");
        assert_eq!(c.entity_count(), 0);
    }

    #[test]
    fn update_entity_transform() {
        let mut c = ClientWorldCache::new();
        c.spawn_entity("e1".into(), "npc".into(), 0.0, 0.0, 0.0, 0.0);
        c.update_entity_transform("e1", 10.0, 1.0, 20.0, 3.14, 2.0, 0.0, 1.0, 100, 0.033);

        let e = &c.entities["e1"];
        assert!((e.x - 10.0).abs() < f32::EPSILON);
        assert!((e.z - 20.0).abs() < f32::EPSILON);
        assert_eq!(e.frame, 100);
        assert!((e.vx - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_missing_entity_is_noop() {
        let mut c = ClientWorldCache::new();
        // Should not panic
        c.update_entity_transform("ghost", 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 1, 0.033);
        assert_eq!(c.entity_count(), 0);
    }

    // ---------------------------------------------------------------
    // Entity extrapolation
    // ---------------------------------------------------------------

    #[test]
    fn extrapolate_entity_position() {
        let e = CachedEntity {
            entity_id: "e1".into(),
            archetype: "npc".into(),
            x: 10.0,
            y: 0.0,
            z: 20.0,
            rotation_y: 0.0,
            vx: 2.0,
            vy: 0.0,
            vz: -1.0,
            frame: 0,
            dt: 0.033,
        };
        let (ex, ey, ez) = e.extrapolated(0.5);
        assert!((ex - 11.0).abs() < f32::EPSILON);
        assert!((ey - 0.0).abs() < f32::EPSILON);
        assert!((ez - 19.5).abs() < f32::EPSILON);
    }

    #[test]
    fn extrapolate_zero_elapsed() {
        let e = CachedEntity {
            entity_id: "e1".into(),
            archetype: "npc".into(),
            x: 5.0,
            y: 0.0,
            z: 5.0,
            rotation_y: 0.0,
            vx: 100.0,
            vy: 0.0,
            vz: 100.0,
            frame: 0,
            dt: 0.033,
        };
        let (ex, _, ez) = e.extrapolated(0.0);
        assert!((ex - 5.0).abs() < f32::EPSILON);
        assert!((ez - 5.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // Clear / reset
    // ---------------------------------------------------------------

    #[test]
    fn clear_resets_everything() {
        let mut c = ClientWorldCache::new();
        c.activate_chunk("c1".into(), 0, 0, 1, 0, 10.0);
        c.spawn_structure("s1".into(), "rock".into(), 0.0, 0.0, 0.0, 0.0);
        c.spawn_entity("e1".into(), "npc".into(), 0.0, 0.0, 0.0, 0.0);
        c.last_frame = 42;
        c.in_snapshot = true;

        c.clear();

        assert_eq!(c.chunk_count(), 0);
        assert_eq!(c.structure_count(), 0);
        assert_eq!(c.entity_count(), 0);
        assert_eq!(c.last_frame, 0);
        assert!(!c.in_snapshot);
    }

    // ---------------------------------------------------------------
    // Multiple entities / ordering independence
    // ---------------------------------------------------------------

    #[test]
    fn multiple_entities() {
        let mut c = ClientWorldCache::new();
        for i in 0..100 {
            c.spawn_entity(format!("e{i}"), "npc".into(), i as f32, 0.0, 0.0, 0.0);
        }
        assert_eq!(c.entity_count(), 100);

        c.remove_entity("e50");
        assert_eq!(c.entity_count(), 99);

        c.clear();
        assert_eq!(c.entity_count(), 0);
    }
}
