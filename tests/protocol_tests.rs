//! Protocol/config compatibility tests for janet-world expansion fields.

use janet_world::protocol::ChunkActivated;
use janet_world::types::WorldServiceConfig;

#[test]
fn world_service_config_defaults_tile_size_m_to_two_metres() {
    let cfg = WorldServiceConfig::default();
    assert!((cfg.tile_size_m - 2.0).abs() < f32::EPSILON);
}

#[test]
fn chunk_activated_deserializes_legacy_payload_with_defaults() {
    let legacy = serde_json::json!({
        "chunk_id": "0:0",
        "cx": 0,
        "cy": 0,
        "seed": 42,
        "terrain_seed": 42,
        "lod": 0,
        "chunk_size": 32.0
    });

    let parsed: ChunkActivated = serde_json::from_value(legacy).expect("legacy payload should parse");

    assert_eq!(parsed.tile_resolution, 2.0);
    assert_eq!(parsed.terrain_algo_version, "md5_value_noise_v1");
}

#[test]
fn chunk_activated_roundtrip_preserves_expanded_fields() {
    let payload = ChunkActivated {
        chunk_id: "1:2".to_string(),
        cx: 1,
        cy: 2,
        seed: 1337,
        terrain_seed: 1337,
        tile_resolution: 1.5,
        terrain_algo_version: "custom_algo_v2".to_string(),
        lod: 1,
        chunk_size: 64.0,
    };

    let v = serde_json::to_value(&payload).expect("serialize");
    let reparsed: ChunkActivated = serde_json::from_value(v).expect("deserialize");

    assert_eq!(reparsed.chunk_id, "1:2");
    assert_eq!(reparsed.tile_resolution, 1.5);
    assert_eq!(reparsed.terrain_algo_version, "custom_algo_v2");
    assert_eq!(reparsed.lod, 1);
}
