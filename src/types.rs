//! Core world types shared across all modules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use janet_operations::physics::types::ColliderShape;

// ---------------------------------------------------------------------------
// Basic math
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

impl std::fmt::Display for Vec3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.2}, {:.2}, {:.2})", self.x, self.y, self.z)
    }
}

// ---------------------------------------------------------------------------
// Spatial chunking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl CellCoord {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

impl std::fmt::Display for CellCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{},{}]", self.x, self.y, self.z)
    }
}

// ---------------------------------------------------------------------------
// World objects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldObject {
    pub id: String,
    pub kind: String,
    pub position: Vec3,
    pub collider: ColliderShape,
    pub properties: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Stats & config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldStats {
    pub active_cells: usize,
    pub total_objects: usize,
    pub tracked_participants: usize,
    pub total_ticks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldServiceConfig {
    /// Width/height of a single streaming cell in world units.
    pub cell_size: f32,
    /// How many cells to stream around each participant (Chebyshev radius).
    pub activation_radius: i32,
    /// Deterministic noise seed.
    pub world_seed: u64,
    /// Density of tree objects per cell (future use).
    pub tree_density: f32,
    /// Physics integration step size in seconds.
    pub physics_dt: f32,
}

impl Default for WorldServiceConfig {
    fn default() -> Self {
        Self {
            cell_size: 10.0,
            activation_radius: 16,
            world_seed: 42,
            tree_density: 0.02,
            physics_dt: 1.0 / 30.0,
        }
    }
}
