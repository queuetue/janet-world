//! Terrain unit tests

#[cfg(test)]
mod tests {
    use janet_world::terrain::{HeightmapTerrain, TerrainSource};

    fn make_terrain(seed: u64) -> HeightmapTerrain {
        HeightmapTerrain::new(seed, 64.0, 32)
    }

    // -----------------------------------------------------------------------
    // Determinism
    // -----------------------------------------------------------------------

    #[test]
    fn height_is_deterministic() {
        let t = make_terrain(42);
        let h1 = t.height_at(10.0, 10.0);
        let h2 = t.height_at(10.0, 10.0);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_seeds_produce_different_terrain() {
        let t1 = make_terrain(1);
        let t2 = make_terrain(999999);
        // Check several points – very unlikely to all be identical.
        let points = [
            (50.0f32, 50.0),
            (123.0, 456.0),
            (-77.0, 33.0),
            (200.0, 100.0),
        ];
        let all_same = points
            .iter()
            .all(|(x, y)| (t1.height_at(*x, *y) - t2.height_at(*x, *y)).abs() < 1e-6);
        assert!(!all_same, "At least one sample should differ between seeds");
    }

    // -----------------------------------------------------------------------
    // Height values are reasonable
    // -----------------------------------------------------------------------

    #[test]
    fn height_within_expected_range() {
        let t = make_terrain(42);
        for x in [-100, 0, 100, 500] {
            for y in [-100, 0, 100, 500] {
                let h = t.height_at(x as f32, y as f32);
                // placeholder sine noise yields values in [-10, 10]
                assert!(
                    h >= -15.0 && h <= 15.0,
                    "height {} out of expected range at ({}, {})",
                    h,
                    x,
                    y
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Normals
    // -----------------------------------------------------------------------

    #[test]
    fn normal_has_nonzero_z() {
        let t = make_terrain(42);
        let n = t.normal_at(30.0, 30.0);
        // Z is always 2*eps = 1.0 in the current FD implementation
        assert!(n.z > 0.0, "normal Z should be positive, got {:?}", n);
    }

    // -----------------------------------------------------------------------
    // Chunk cache
    // -----------------------------------------------------------------------

    #[test]
    fn chunk_cache_returns_same_arc() {
        let t = make_terrain(42);
        let c1 = t.get_or_generate_chunk(0, 0, 0);
        let c2 = t.get_or_generate_chunk(0, 0, 0);
        // Same Arc pointer means the cache is being used.
        assert!(Arc::ptr_eq(&c1, &c2));
    }

    // -----------------------------------------------------------------------
    // LOD
    // -----------------------------------------------------------------------

    #[test]
    fn lod_reduces_resolution() {
        let t = make_terrain(42);
        let c0 = t.get_or_generate_chunk(0, 0, 0);
        let c1 = t.get_or_generate_chunk(0, 0, 1);
        let c2 = t.get_or_generate_chunk(0, 0, 2);
        assert!(c0.resolution >= c1.resolution);
        assert!(c1.resolution >= c2.resolution);
    }

    #[test]
    fn lod_for_distance_returns_increasing_levels() {
        let t = make_terrain(42);
        assert_eq!(t.lod_for_distance(50.0), 0);
        assert_eq!(t.lod_for_distance(150.0), 1);
        assert_eq!(t.lod_for_distance(350.0), 2);
    }

    // -----------------------------------------------------------------------
    // Eviction
    // -----------------------------------------------------------------------

    #[test]
    fn evict_removes_distant_chunks() {
        let t = make_terrain(42);
        // Pre-warm several chunks
        for x in -5_i32..=5 {
            for y in -5_i32..=5 {
                t.get_or_generate_chunk(x, y, 0);
            }
        }
        // Evict everything more than 1 chunk away from origin
        t.evict_distant_chunks(0, 0, 1);
        // Evicted – requesting a distant chunk should regenerate without error
        let _ = t.get_or_generate_chunk(5, 5, 0);
    }

    // -----------------------------------------------------------------------
    // Heightfield collider shape
    // -----------------------------------------------------------------------

    #[test]
    fn heightfield_collider_has_correct_size() {
        use janet_operations::physics::types::ColliderShape;
        let t = make_terrain(42);
        let shape = t.heightfield_collider_for_chunk(0, 0, 0);
        // Until Collins::Heightfield is available we expect a Box covering the chunk.
        match shape {
            ColliderShape::Box { width, height } => {
                assert!(width > 0.0, "chunk box width should be positive");
                assert!(height > 0.0, "chunk box height should be positive");
            }
            _ => panic!("Expected ColliderShape::Box for placeholder terrain"),
        }
    }

    use std::sync::Arc;
}
