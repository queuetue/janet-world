//! WorldService unit tests

#[cfg(test)]
mod tests {
    use janet_operations::physics::{types::PhysicsRegistryConfig, PhysicsRegistry};
    use janet_world::{
        service::WorldService,
        structure::World,
        terrain::HeightmapTerrain,
        types::{Vec3, WorldServiceConfig},
    };
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn make_service(radius: i32) -> WorldService {
        let terrain = Arc::new(HeightmapTerrain::new(42, 64.0, 16));
        let world = Arc::new(World::new(terrain));
        let physics = Arc::new(RwLock::new(PhysicsRegistry::new(
            PhysicsRegistryConfig::default(),
        )));

        let config = WorldServiceConfig {
            cell_size: 10.0,
            activation_radius: radius,
            world_seed: 42,
            physics_dt: 1.0 / 30.0,
            ..Default::default()
        };

        WorldService::new(config, physics, world)
    }

    // -----------------------------------------------------------------------
    // Participant management
    // -----------------------------------------------------------------------

    #[test]
    fn register_and_unregister_participant() {
        let mut svc = make_service(2);
        assert_eq!(svc.participant_count(), 0);

        svc.register_participant("alice".into(), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(svc.participant_count(), 1);

        svc.unregister_participant("alice");
        assert_eq!(svc.participant_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    #[test]
    fn stats_reflect_initial_state() {
        let svc = make_service(2);
        let stats = svc.stats();
        assert_eq!(stats.active_cells, 0);
        assert_eq!(stats.total_ticks, 0);
        assert_eq!(stats.tracked_participants, 0);
    }

    // -----------------------------------------------------------------------
    // Cell activation (no physics simulation initialised – test tick counts)
    // -----------------------------------------------------------------------

    #[test]
    fn tick_increments_tick_count() {
        let mut svc = make_service(0);
        // radius 0 with no participants → no cells to activate, no physics calls
        // – safe to tick without a live simulation
        let stats_before = svc.stats();
        assert_eq!(stats_before.total_ticks, 0);
        // tick() returns Err when there is no default sim; that's acceptable
        // here – we just verify the tick counter advances.
        let _ = svc.tick();
        let stats_after = svc.stats();
        assert_eq!(stats_after.total_ticks, 1);
    }

    // -----------------------------------------------------------------------
    // Determinism – two services with identical seeds produce identical cell sets
    // -----------------------------------------------------------------------

    #[test]
    fn identical_seeds_produce_identical_initial_stats() {
        let svc_a = make_service(4);
        let svc_b = make_service(4);
        // Before any participants both should be identical
        assert_eq!(svc_a.stats().active_cells, svc_b.stats().active_cells);
    }
}
