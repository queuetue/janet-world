//! World Service (3D Hardened Foundation)
//!
//! This version promotes the world model to true 3D.
//! - All positions are Vec3
//! - Cells are 3D chunked
//! - Terrain is first-class
//! - Static colliders are volumetric (no 2D circles)
//!
//! This is now suitable for heightmaps + structure meshes.

use janet_operations::physics::types::ColliderShape;
use janet_operations::physics::{types::BodyParams, PhysicsRegistry};
use log::{debug, warn};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// -----------------------------------------------------------------------------
// Basic Math Types
// -----------------------------------------------------------------------------

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
}

// -----------------------------------------------------------------------------
// Terrain + Structure Abstractions
// -----------------------------------------------------------------------------

pub trait TerrainSource: Send + Sync {
    fn height_at(&self, x: f32, y: f32) -> f32;
    fn normal_at(&self, x: f32, y: f32) -> Vec3;
}

// -----------------------------------------------------------------------------
// Heightmap Terrain Implementation (Chunked + Cached + LOD + Collider Support)
// -----------------------------------------------------------------------------

use std::collections::hash_map::Entry;

pub struct HeightmapTerrain {
    pub seed: u64,
    pub chunk_size: f32,
    pub base_resolution: usize,
    cache: RwLock<HashMap<(i32, i32, u8), Arc<HeightChunk>>>,
}

pub struct HeightChunk {
    pub heights: Vec<f32>,
    pub resolution: usize,
    pub world_origin_x: f32,
    pub world_origin_y: f32,
    pub cell_size: f32,
}

impl HeightmapTerrain {
    pub fn new(seed: u64, chunk_size: f32, base_resolution: usize) -> Self {
        Self {
            seed,
            chunk_size,
            base_resolution,
            cache: RwLock::new(HashMap::new()),
        }
    }

    fn chunk_coord(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.chunk_size).floor() as i32,
            (y / self.chunk_size).floor() as i32,
        )
    }

    fn lod_for_distance(&self, distance: f32) -> u8 {
        if distance < 100.0 {
            0
        } else if distance < 300.0 {
            1
        } else {
            2
        }
    }

    pub fn heightfield_collider_for_chunk(&self, cx: i32, cy: i32, lod: u8) -> ColliderShape {
        let chunk = self.get_or_generate_chunk(cx, cy, lod);

        ColliderShape::Heightfield {
            heights: chunk.heights.clone(),
            rows: chunk.resolution,
            cols: chunk.resolution,
            scale_x: chunk.cell_size,
            scale_y: chunk.cell_size,
        }
    }

    fn get_or_generate_chunk(&self, cx: i32, cy: i32, lod: u8) -> Arc<HeightChunk> {
        let mut cache = self.cache.write();
        match cache.entry((cx, cy, lod)) {
            Entry::Occupied(e) => e.get().clone(),
            Entry::Vacant(v) => {
                let chunk = Arc::new(self.generate_chunk(cx, cy, lod));
                v.insert(chunk.clone());
                chunk
            }
        }
    }

    fn generate_chunk(&self, cx: i32, cy: i32, lod: u8) -> HeightChunk {
        let resolution = (self.base_resolution >> lod).max(4);
        let cell_size = self.chunk_size / resolution as f32;

        let world_origin_x = cx as f32 * self.chunk_size;
        let world_origin_y = cy as f32 * self.chunk_size;

        let mut heights = Vec::with_capacity(resolution * resolution);

        for y in 0..resolution {
            for x in 0..resolution {
                let wx = world_origin_x + x as f32 * cell_size;
                let wy = world_origin_y + y as f32 * cell_size;
                heights.push(self.sample_noise(wx, wy));
            }
        }

        HeightChunk {
            heights,
            resolution,
            world_origin_x,
            world_origin_y,
            cell_size,
        }
    }

    fn sample_noise(&self, x: f32, y: f32) -> f32 {
        let scale = 0.01;
        ((x * scale).sin() * (y * scale).cos()) * 10.0
    }
}

impl TerrainSource for HeightmapTerrain {
    fn height_at(&self, x: f32, y: f32) -> f32 {
        let (cx, cy) = self.chunk_coord(x, y);
        let chunk = self.get_or_generate_chunk(cx, cy, 0);

        let local_x = x - chunk.world_origin_x;
        let local_y = y - chunk.world_origin_y;

        let gx = (local_x / chunk.cell_size).clamp(0.0, (chunk.resolution - 1) as f32);
        let gy = (local_y / chunk.cell_size).clamp(0.0, (chunk.resolution - 1) as f32);

        let ix = gx.floor() as usize;
        let iy = gy.floor() as usize;

        chunk.heights[iy * chunk.resolution + ix]
    }

    fn normal_at(&self, x: f32, y: f32) -> Vec3 {
        let eps = 0.5;
        let h_l = self.height_at(x - eps, y);
        let h_r = self.height_at(x + eps, y);
        let h_d = self.height_at(x, y - eps);
        let h_u = self.height_at(x, y + eps);

        Vec3::new(h_l - h_r, h_d - h_u, 2.0 * eps)
    }
}

// -----------------------------------------------------------------------------
// Structures
// -----------------------------------------------------------------------------

pub struct StructureInstance {
    pub id: String,
    pub position: Vec3,
    pub collider: ColliderShape,
}

pub struct StructureRegistry {
    pub instances: HashMap<String, StructureInstance>,
}

pub struct World {
    pub terrain: Arc<dyn TerrainSource>,
    pub structures: StructureRegistry,
}

// -----------------------------------------------------------------------------
// Spatial Chunking
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone)]
pub struct WorldServiceConfig {
    pub cell_size: f32,
    pub activation_radius: i32,
    pub world_seed: u64,
    pub tree_density: f32,
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

// -----------------------------------------------------------------------------
// World Objects
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldObject {
    pub id: String,
    pub kind: String,
    pub position: Vec3,
    pub collider: ColliderShape,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldStats {
    pub active_cells: usize,
    pub total_objects: usize,
    pub tracked_participants: usize,
    pub total_ticks: u64,
}

// -----------------------------------------------------------------------------
// World Service
// -----------------------------------------------------------------------------

pub struct WorldService {
    config: WorldServiceConfig,
    active_cells: HashSet<CellCoord>,
    terrain_bodies: HashMap<CellCoord, String>,
    cell_objects: HashMap<CellCoord, Vec<String>>,
    world_objects: HashMap<String, WorldObject>,
    participant_positions: HashMap<String, Vec3>,
    physics_registry: Arc<RwLock<PhysicsRegistry>>,
    world: Arc<World>,
    tick_count: u64,
}

impl WorldService {
    pub fn new(
        config: WorldServiceConfig,
        physics_registry: Arc<RwLock<PhysicsRegistry>>,
        world: Arc<World>,
    ) -> Self {
        Self {
            config,
            active_cells: HashSet::new(),
            terrain_bodies: HashMap::new(),
            cell_objects: HashMap::new(),
            world_objects: HashMap::new(),
            participant_positions: HashMap::new(),
            physics_registry,
            world,
            tick_count: 0,
        }
    }

    pub fn register_participant(&mut self, id: String, position: Vec3) {
        self.participant_positions.insert(id, position);
    }

    pub fn tick(&mut self) -> janet::Result<()> {
        self.tick_count += 1;
        self.sync_positions_from_registry();

        let desired = self.compute_active_cells();

        let to_deactivate: Vec<_> = self.active_cells.difference(&desired).cloned().collect();

        for c in to_deactivate {
            self.deactivate_cell(&c)?;
        }

        let to_activate: Vec<_> = desired.difference(&self.active_cells).cloned().collect();

        for c in to_activate {
            self.activate_cell(c)?;
        }

        Ok(())
    }

    fn compute_active_cells(&self) -> HashSet<CellCoord> {
        let mut set = HashSet::new();
        let r = self.config.activation_radius;

        for pos in self.participant_positions.values() {
            let cx = (pos.x / self.config.cell_size).floor() as i32;
            let cy = (pos.y / self.config.cell_size).floor() as i32;

            for dx in -r..=r {
                for dy in -r..=r {
                    set.insert(CellCoord {
                        x: cx + dx,
                        y: cy + dy,
                        z: 0,
                    });
                }
            }
        }

        set
    }

    fn activate_cell(&mut self, coord: CellCoord) -> janet::Result<()> {
        if self.active_cells.contains(&coord) {
            return Ok(());
        }

        let mut registry = self.physics_registry.write();
        let sim = registry
            .default_simulation_mut()
            .ok_or_else(|| janet::JanetError::Other("No default physics simulation".into()))?;

        // Terrain streaming
        if let Some(heightmap) = self
            .world
            .terrain
            .as_any()
            .downcast_ref::<HeightmapTerrain>()
        {
            let id = format!("terrain.{}.{}", coord.x, coord.y);
            let collider = heightmap.heightfield_collider_for_chunk(coord.x, coord.y, 0);

            sim.register_body(
                id.clone(),
                BodyParams::Static {
                    shape: collider,
                    position: Vec3::new(
                        coord.x as f32 * self.config.cell_size,
                        coord.y as f32 * self.config.cell_size,
                        0.0,
                    ),
                    rotation: 0.0,
                },
            )?;

            self.terrain_bodies.insert(coord, id);
        }

        self.active_cells.insert(coord);
        Ok(())
    }

    fn deactivate_cell(&mut self, coord: &CellCoord) -> janet::Result<()> {
        if let Some(id) = self.terrain_bodies.remove(coord) {
            let mut registry = self.physics_registry.write();
            if let Some(sim) = registry.default_simulation_mut() {
                let _ = sim.unregister_body(&id);
            }
        }

        self.active_cells.remove(coord);
        Ok(())
    }

    fn sync_positions_from_registry(&mut self) {
        let registry = self.physics_registry.read();
        let Some(sim) = registry.default_simulation() else {
            return;
        };

        for (id, pos) in self.participant_positions.clone() {
            if let Ok(transform) = sim.get_transform(&id) {
                self.participant_positions.insert(id, transform.position);
            } else {
                self.participant_positions.insert(id, pos);
            }
        }
    }
}
