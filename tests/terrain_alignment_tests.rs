//! Terrain alignment integration test vectors.

use janet_world::terrain::sample_canonical_tile;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Sample {
    lx: i32,
    ly: i32,
    terrain: String,
    elevation: f32,
    resources: f32,
    hazard: f32,
}

#[derive(Debug, Deserialize)]
struct VectorCase {
    seed: u64,
    cx: i32,
    cy: i32,
    samples: Vec<Sample>,
}

#[test]
fn canonical_terrain_matches_pinned_python_vectors() {
    let raw = std::fs::read_to_string("tests/terrain_alignment_vectors.json")
        .expect("read terrain_alignment_vectors.json");
    let vectors: Vec<VectorCase> = serde_json::from_str(&raw).expect("parse vectors json");

    for case in vectors {
        for expected in case.samples {
            let actual =
                sample_canonical_tile(case.seed, case.cx, case.cy, expected.lx, expected.ly);
            assert_eq!(
                actual.terrain, expected.terrain,
                "terrain mismatch seed={} chunk=({}, {}) tile=({}, {})",
                case.seed, case.cx, case.cy, expected.lx, expected.ly
            );
            assert!(
                (actual.elevation - expected.elevation).abs() <= 0.0001,
                "elevation mismatch seed={} chunk=({}, {}) tile=({}, {}): got {} expected {}",
                case.seed,
                case.cx,
                case.cy,
                expected.lx,
                expected.ly,
                actual.elevation,
                expected.elevation
            );
            assert!(
                (actual.resources - expected.resources).abs() <= 0.0001,
                "resources mismatch seed={} chunk=({}, {}) tile=({}, {}): got {} expected {}",
                case.seed,
                case.cx,
                case.cy,
                expected.lx,
                expected.ly,
                actual.resources,
                expected.resources
            );
            assert!(
                (actual.hazard - expected.hazard).abs() <= 0.0001,
                "hazard mismatch seed={} chunk=({}, {}) tile=({}, {}): got {} expected {}",
                case.seed,
                case.cx,
                case.cy,
                expected.lx,
                expected.ly,
                actual.hazard,
                expected.hazard
            );
        }
    }
}
