//! WorldService – streaming, cell activation/deactivation, terrain physics bodies.

use crate::protocol::{
    ChunkActivated, ChunkDeactivated, EntitySpawned, EntityTransform, StructureSpawned,
    WorldSnapshot,
};
use crate::structure::World;
use crate::terrain::HeightmapTerrain;
use crate::types::{CellCoord, Vec3, WorldObject, WorldServiceConfig, WorldStats};
use janet_operations::physics::{types::BodyParams, PhysicsRegistry};
use log::{debug, warn};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Tick result
// ---------------------------------------------------------------------------

/// Events produced by a single [`WorldService::tick`] call.
///
/// Callers (typically [`WorldBusAgent`]) publish these to the bus.
pub struct TickEvents {
    /// The tick counter that produced this set of events.
    pub tick: u64,
    /// Chunks that were activated this tick.
    pub activated: Vec<ChunkActivated>,
    /// Chunks that were deactivated this tick.
    pub deactivated: Vec<ChunkDeactivated>,
    /// Authoritative transforms for every tracked participant/entity.
    pub entity_transforms: Vec<EntityTransform>,
}

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

    // -----------------------------------------------------------------------
    // Participant management
    // -----------------------------------------------------------------------

    pub fn register_participant(&mut self, id: String, position: Vec3) {
        self.participant_positions.insert(id, position);
    }

    pub fn unregister_participant(&mut self, id: &str) {
        self.participant_positions.remove(id);
    }

    pub fn participant_count(&self) -> usize {
        self.participant_positions.len()
    }

    // -----------------------------------------------------------------------
    // Main tick
    // -----------------------------------------------------------------------

    /// Advance simulation by one tick.
    ///
    /// Returns [`TickEvents`] describing every state change that occurred so
    /// the bus agent can publish the corresponding protocol messages.
    pub fn tick(&mut self) -> janet::Result<TickEvents> {
        self.tick_count += 1;
        self.sync_positions_from_registry();

        let desired = self.compute_active_cells();

        let mut activated = Vec::new();
        let mut deactivated = Vec::new();

        let to_deactivate: Vec<_> = self.active_cells.difference(&desired).cloned().collect();
        for c in to_deactivate {
            deactivated.push(self.deactivate_cell(&c)?);
        }

        let to_activate: Vec<_> = desired.difference(&self.active_cells).cloned().collect();
        for c in to_activate {
            if let Some(ev) = self.activate_cell(c)? {
                activated.push(ev);
            }
        }

        let entity_transforms = self.collect_entity_transforms();

        Ok(TickEvents {
            tick: self.tick_count,
            activated,
            deactivated,
            entity_transforms,
        })
    }

    // -----------------------------------------------------------------------
    // Snapshot
    // -----------------------------------------------------------------------

    /// Build a full-state [`WorldSnapshot`] for a reconnecting client.
    pub fn build_snapshot(&self, _session: &str) -> WorldSnapshot {
        // Active chunks
        let active_chunks = self
            .active_cells
            .iter()
            .map(|coord| {
                let (seed, chunk_size) = self
                    .world
                    .terrain
                    .as_any()
                    .downcast_ref::<HeightmapTerrain>()
                    .map(|hm| (hm.seed, hm.chunk_size))
                    .unwrap_or((0, self.config.cell_size));

                ChunkActivated {
                    chunk_id: format!("{}:{}", coord.x, coord.y),
                    cx: coord.x,
                    cy: coord.y,
                    seed,
                    lod: 0,
                    chunk_size,
                }
            })
            .collect();

        // Structures (all; a real impl might page by view radius)
        let structures = self
            .world
            .structures
            .query_rect(
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            )
            .into_iter()
            .map(|s| StructureSpawned {
                structure_id: s.id.clone(),
                type_id: s
                    .metadata
                    .get("type_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                x: s.position.x,
                y: s.position.y,
                z: s.position.z,
                rotation_y: 0.0,
                metadata: serde_json::Value::Object(
                    s.metadata
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
            })
            .collect();

        // Participants as entity stubs
        let entities = self
            .participant_positions
            .iter()
            .map(|(id, pos)| EntitySpawned {
                entity_id: id.clone(),
                archetype: "participant".into(),
                x: pos.x,
                y: pos.y,
                z: pos.z,
                rotation_y: 0.0,
                metadata: serde_json::Value::Null,
            })
            .collect();

        WorldSnapshot {
            active_chunks,
            structures,
            entities,
        }
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub fn stats(&self) -> WorldStats {
        WorldStats {
            active_cells: self.active_cells.len(),
            total_objects: self.world_objects.len(),
            tracked_participants: self.participant_positions.len(),
            total_ticks: self.tick_count,
        }
    }

    // -----------------------------------------------------------------------
    // Cell computation
    // -----------------------------------------------------------------------

    fn compute_active_cells(&self) -> HashSet<CellCoord> {
        let mut set = HashSet::new();
        let r = self.config.activation_radius;

        for pos in self.participant_positions.values() {
            let cx = (pos.x / self.config.cell_size).floor() as i32;
            let cy = (pos.y / self.config.cell_size).floor() as i32;

            for dx in -r..=r {
                for dy in -r..=r {
                    set.insert(CellCoord::new(cx + dx, cy + dy, 0));
                }
            }
        }

        set
    }

    fn activate_cell(&mut self, coord: CellCoord) -> janet::Result<Option<ChunkActivated>> {
        if self.active_cells.contains(&coord) {
            return Ok(None);
        }

        let mut registry = self.physics_registry.write();
        let sim = registry
            .default_simulation_mut()
            .ok_or_else(|| janet::JanetError::Other("No default physics simulation".into()))?;

        // Terrain streaming – downcast to HeightmapTerrain for heightfield support.
        if let Some(hm) = self
            .world
            .terrain
            .as_any()
            .downcast_ref::<HeightmapTerrain>()
        {
            let body_id = format!("terrain.{}.{}", coord.x, coord.y);
            let collider = hm.heightfield_collider_for_chunk(coord.x, coord.y, 0);

            sim.register_body(
                body_id.clone(),
                BodyParams::Static {
                    shape: collider,
                    position: (
                        coord.x as f32 * self.config.cell_size,
                        coord.y as f32 * self.config.cell_size,
                    ),
                    rotation: 0.0,
                },
            )?;

            debug!("Activated terrain cell {}", coord);
            self.terrain_bodies.insert(coord, body_id);
        }

        self.active_cells.insert(coord);

        // Build protocol event — grab seed from terrain if HeightmapTerrain.
        let (seed, chunk_size) = self
            .world
            .terrain
            .as_any()
            .downcast_ref::<HeightmapTerrain>()
            .map(|hm| (hm.seed, hm.chunk_size))
            .unwrap_or((0, self.config.cell_size));

        let chunk_id = format!("{}:{}", coord.x, coord.y);
        Ok(Some(ChunkActivated {
            chunk_id,
            cx: coord.x,
            cy: coord.y,
            seed,
            lod: 0,
            chunk_size,
        }))
    }

    fn deactivate_cell(&mut self, coord: &CellCoord) -> janet::Result<ChunkDeactivated> {
        if let Some(id) = self.terrain_bodies.remove(coord) {
            let mut registry = self.physics_registry.write();
            if let Some(sim) = registry.default_simulation_mut() {
                if let Err(e) = sim.unregister_body(&id) {
                    warn!("Failed to unregister terrain body {}: {}", id, e);
                }
            }
        }

        if let Some(object_ids) = self.cell_objects.remove(coord) {
            let mut registry = self.physics_registry.write();
            if let Some(sim) = registry.default_simulation_mut() {
                for id in &object_ids {
                    if let Err(e) = sim.unregister_body(id) {
                        warn!("Failed to unregister object body {}: {}", id, e);
                    }
                }
            }
        }

        debug!("Deactivated cell {}", coord);
        self.active_cells.remove(coord);

        let chunk_id = format!("{}:{}", coord.x, coord.y);
        Ok(ChunkDeactivated { chunk_id })
    }

    // -----------------------------------------------------------------------
    // Entity transforms
    // -----------------------------------------------------------------------

    /// Collect authoritative transforms for every tracked participant.
    ///
    /// These are published each tick so clients can interpolate movement.
    fn collect_entity_transforms(&self) -> Vec<EntityTransform> {
        self.participant_positions
            .iter()
            .map(|(id, pos)| EntityTransform {
                entity_id: id.clone(),
                x: pos.x,
                y: pos.y,
                z: pos.z,
                rotation_y: 0.0,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
                dt: 0.0,
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Physics sync
    // -----------------------------------------------------------------------

    fn sync_positions_from_registry(&mut self) {
        let registry = self.physics_registry.read();
        let Some(sim) = registry.default_simulation() else {
            return;
        };

        let ids: Vec<_> = self.participant_positions.keys().cloned().collect();
        for id in ids {
            if let Ok(transform) = sim.get_transform(&id) {
                let (px, py) = transform.position;
                self.participant_positions
                    .insert(id, Vec3::new(px, py, 0.0));
            }
        }
    }
}
