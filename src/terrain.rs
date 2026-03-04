//! Terrain subsystem: TerrainSource trait, HeightmapTerrain implementation,
//! chunk cache, LOD generation, and heightfield collider construction.

use crate::types::Vec3;
use janet_operations::physics::types::ColliderShape;
use md5;
use parking_lot::RwLock;
use std::any::Any;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

const CANONICAL_TILE_SIZE: i32 = 16;

#[derive(Debug, Clone)]
pub struct CanonicalTileSample {
    pub terrain: String,
    pub elevation: f32,
    pub resources: f32,
    pub hazard: f32,
}

fn classify_terrain(elevation: f64) -> &'static str {
    if elevation < 0.18 {
        "water"
    } else if elevation < 0.28 {
        "sand"
    } else if elevation < 0.32 {
        "swamp"
    } else if elevation < 0.58 {
        "grass"
    } else if elevation < 0.72 {
        "forest"
    } else if elevation < 0.84 {
        "rock"
    } else if elevation < 0.94 {
        "snow"
    } else {
        "desert"
    }
}

fn hash_float(ix: i32, iy: i32, salt: u64) -> f64 {
    let key = format!("{}:{}:{}", ix, iy, salt);
    let digest = md5::compute(key.as_bytes());
    let low = ((digest.0[14] as u16) << 8) | digest.0[15] as u16;
    low as f64 / 65535.0
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smooth_step(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn smooth_noise(wx: f64, wy: f64, scale: f64, salt: u64) -> f64 {
    let sx = wx * scale;
    let sy = wy * scale;
    let ix = sx.floor() as i32;
    let iy = sy.floor() as i32;
    let fx = smooth_step(sx - ix as f64);
    let fy = smooth_step(sy - iy as f64);
    let v00 = hash_float(ix, iy, salt);
    let v10 = hash_float(ix + 1, iy, salt);
    let v01 = hash_float(ix, iy + 1, salt);
    let v11 = hash_float(ix + 1, iy + 1, salt);
    lerp(lerp(v00, v10, fx), lerp(v01, v11, fx), fy)
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

fn elevation(wx: f64, wy: f64, seed: u64) -> f64 {
    clamp01(
        0.50 * smooth_noise(wx, wy, 0.04, seed ^ 0x1111)
            + 0.30 * smooth_noise(wx, wy, 0.10, seed ^ 0x2222)
            + 0.20 * smooth_noise(wx, wy, 0.25, seed ^ 0x3333),
    )
}

fn resources(wx: f64, wy: f64, seed: u64) -> f64 {
    clamp01(
        0.65 * smooth_noise(wx, wy, 0.07, seed ^ 0x4444)
            + 0.35 * smooth_noise(wx, wy, 0.18, seed ^ 0x5555),
    )
}

fn hazard(wx: f64, wy: f64, seed: u64) -> f64 {
    smooth_noise(wx, wy, 0.15, seed ^ 0x6666)
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

pub fn sample_canonical_tile(seed: u64, cx: i32, cy: i32, lx: i32, ly: i32) -> CanonicalTileSample {
    let wx = (cx * CANONICAL_TILE_SIZE + lx) as f64;
    let wy = (cy * CANONICAL_TILE_SIZE + ly) as f64;

    let elev = elevation(wx, wy, seed);
    let mut terrain = classify_terrain(elev).to_string();
    let mut res = resources(wx, wy, seed);
    let mut haz = hazard(wx, wy, seed);

    if terrain == "water" {
        res = 0.0;
        haz = haz.max(0.35);
        terrain = "water".to_string();
    }

    CanonicalTileSample {
        terrain,
        elevation: round4(elev) as f32,
        resources: round4(res) as f32,
        hazard: round4(haz) as f32,
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Anything that can provide a terrain height and surface normal.
///
/// The `as_any` method enables downcasting from `Arc<dyn TerrainSource>` to a
/// concrete type (e.g. [`HeightmapTerrain`]) when heightfield colliders are
/// needed.
pub trait TerrainSource: Send + Sync {
    fn height_at(&self, x: f32, y: f32) -> f32;
    fn normal_at(&self, x: f32, y: f32) -> Vec3;

    /// Downcast support (implement by returning `self`).
    fn as_any(&self) -> &dyn Any;
}

// ---------------------------------------------------------------------------
// Height chunk
// ---------------------------------------------------------------------------

pub struct HeightChunk {
    pub heights: Vec<f32>,
    pub resolution: usize,
    pub world_origin_x: f32,
    pub world_origin_y: f32,
    pub cell_size: f32,
}

// ---------------------------------------------------------------------------
// Heightmap terrain
// ---------------------------------------------------------------------------

pub struct HeightmapTerrain {
    pub seed: u64,
    /// World-space width/height of a single terrain chunk.
    pub chunk_size: f32,
    /// Sample resolution at LOD 0 (halved per LOD level).
    pub base_resolution: usize,
    cache: RwLock<HashMap<(i32, i32, u8), Arc<HeightChunk>>>,
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

    pub fn chunk_coord(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.chunk_size).floor() as i32,
            (y / self.chunk_size).floor() as i32,
        )
    }

    pub fn lod_for_distance(&self, distance: f32) -> u8 {
        if distance < 100.0 {
            0
        } else if distance < 300.0 {
            1
        } else {
            2
        }
    }

    /// Build a `ColliderShape::Heightfield` for a chunk at the given LOD.
    pub fn heightfield_collider_for_chunk(&self, cx: i32, cy: i32, lod: u8) -> ColliderShape {
        let chunk = self.get_or_generate_chunk(cx, cy, lod);

        // TODO(Phase 1): Replace with ColliderShape::Heightfield once the physics
        // engine exposes a 3D heightfield variant.  For now use a flat Box that
        // covers the chunk footprint.
        let _ = lod; // suppress unused-variable lint until variant is added
        ColliderShape::Box {
            width: chunk.resolution as f32 * chunk.cell_size,
            height: chunk.resolution as f32 * chunk.cell_size,
        }
    }

    // -----------------------------------------------------------------------
    // Cache helpers
    // -----------------------------------------------------------------------

    pub fn get_or_generate_chunk(&self, cx: i32, cy: i32, lod: u8) -> Arc<HeightChunk> {
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

    /// Evict every chunk whose (cx, cy) chunk-centre is further than
    /// `max_chunks` cells from `origin` in Chebyshev distance.
    pub fn evict_distant_chunks(&self, origin_cx: i32, origin_cy: i32, max_chunks: i32) {
        let mut cache = self.cache.write();
        cache.retain(|(cx, cy, _lod), _| {
            let dx = (cx - origin_cx).abs();
            let dy = (cy - origin_cy).abs();
            dx <= max_chunks && dy <= max_chunks
        });
    }

    // -----------------------------------------------------------------------
    // Generation
    // -----------------------------------------------------------------------

    fn generate_chunk(&self, cx: i32, cy: i32, lod: u8) -> HeightChunk {
        let resolution = (self.base_resolution >> lod).max(4);
        let cell_size = self.chunk_size / resolution as f32;
        let world_origin_x = cx as f32 * self.chunk_size;
        let world_origin_y = cy as f32 * self.chunk_size;

        let mut heights = Vec::with_capacity(resolution * resolution);
        for row in 0..resolution {
            for col in 0..resolution {
                let wx = world_origin_x + col as f32 * cell_size;
                let wy = world_origin_y + row as f32 * cell_size;
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

    /// Canonical deterministic elevation noise aligned with Python world generator.
    fn sample_noise(&self, x: f32, y: f32) -> f32 {
        elevation(x as f64, y as f64, self.seed) as f32
    }
}

// ---------------------------------------------------------------------------
// TerrainSource impl
// ---------------------------------------------------------------------------

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
        // Finite-difference gradient (replace with analytical in Phase 1.3)
        let eps = 0.5;
        let h_l = self.height_at(x - eps, y);
        let h_r = self.height_at(x + eps, y);
        let h_d = self.height_at(x, y - eps);
        let h_u = self.height_at(x, y + eps);
        Vec3::new(h_l - h_r, h_d - h_u, 2.0 * eps)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
