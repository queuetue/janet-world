//! Structure subsystem: static mesh instances and their registry,
//! plus the top-level `World` data container.

use crate::terrain::TerrainSource;
use crate::types::Vec3;
use janet_operations::physics::types::ColliderShape;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Structure instance
// ---------------------------------------------------------------------------

/// A single static structure placed in the world (building, rock, barrier …).
#[derive(Debug)]
pub struct StructureInstance {
    /// Globally unique identifier for the structure.
    pub id: String,
    /// World-space origin of the structure.
    pub position: Vec3,
    /// Approximate bounding half-extents used for per-chunk bucketing.
    pub bounds_radius: f32,
    /// Physics collider shape (mesh or convex hull).
    pub collider: ColliderShape,
    /// Arbitrary metadata (asset path, tags, …).
    pub metadata: HashMap<String, serde_json::Value>,
}

impl StructureInstance {
    pub fn new(id: impl Into<String>, position: Vec3, collider: ColliderShape) -> Self {
        Self {
            id: id.into(),
            position,
            bounds_radius: 5.0,
            collider,
            metadata: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Holds all static structures placed in the world.
///
/// Future: bucket by spatial grid so streaming can query per-chunk.
pub struct StructureRegistry {
    instances: HashMap<String, StructureInstance>,
}

impl StructureRegistry {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
        }
    }

    pub fn insert(&mut self, structure: StructureInstance) {
        self.instances.insert(structure.id.clone(), structure);
    }

    pub fn remove(&mut self, id: &str) -> Option<StructureInstance> {
        self.instances.remove(id)
    }

    pub fn get(&self, id: &str) -> Option<&StructureInstance> {
        self.instances.get(id)
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Return all structures whose bounding circle overlaps the given world
    /// rectangle (used during chunk activation for selective streaming).
    pub fn query_rect(
        &self,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
    ) -> Vec<&StructureInstance> {
        self.instances
            .values()
            .filter(|s| {
                let r = s.bounds_radius;
                s.position.x + r >= min_x
                    && s.position.x - r <= max_x
                    && s.position.y + r >= min_y
                    && s.position.y - r <= max_y
            })
            .collect()
    }
}

impl Default for StructureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// World (data container)
// ---------------------------------------------------------------------------

/// The immutable world data layer.  `WorldService` streams it into physics.
pub struct World {
    pub terrain: Arc<dyn TerrainSource>,
    pub structures: StructureRegistry,
}

impl World {
    pub fn new(terrain: Arc<dyn TerrainSource>) -> Self {
        Self {
            terrain,
            structures: StructureRegistry::new(),
        }
    }
}
