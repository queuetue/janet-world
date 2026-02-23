//! janet-world-server binary
//!
//! Starts the world engine and connects it to the janet bus as an
//! external physics participant.
//!
//! ## Configuration (env / TOML via `config` crate)
//!
//! | Key                        | Default             | Description                    |
//! |----------------------------|---------------------|--------------------------------|
//! | `WORLD_SESSION`            | `default`           | Janet session name             |
//! | `WORLD_PARTICIPANT_ID`     | `world-service`     | Bus participant ID             |
//! | `WORLD_ENDPOINT`           | `nats://localhost:4222` | Transport endpoint         |
//! | `WORLD_TICK_RATE_HZ`       | `30`                | Physics / streaming tick rate  |
//! | `WORLD_SEED`               | `42`                | Terrain seed                   |
//! | `WORLD_CELL_SIZE`          | `10.0`              | Streaming cell size (world units) |
//! | `WORLD_ACTIVATION_RADIUS`  | `16`                | Chebyshev streaming radius     |

use anyhow::Result;
use clap::Parser;
use janet_operations::physics::{
    types::{
        OntologyId, PhysicsRegistryConfig, Rapier2DConfig, SimulationMetadata, SimulationType, Tier,
    },
    PhysicsRegistry, Rapier2DSimulation,
};
use janet_world::{
    bus::{WorldBusAgent, WorldBusConfig},
    service::WorldService,
    structure::World,
    terrain::HeightmapTerrain,
    types::WorldServiceConfig,
};
use parking_lot::RwLock;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "janet-world-server", about = "Janet World Engine", version)]
struct Args {
    /// Janet session to join
    #[arg(long, env = "WORLD_SESSION", default_value = "default")]
    session: String,

    /// Bus participant ID
    #[arg(long, env = "WORLD_PARTICIPANT_ID", default_value = "world-service")]
    participant_id: String,

    /// NATS endpoint
    #[arg(long, env = "WORLD_ENDPOINT", default_value = "nats://localhost:4222")]
    endpoint: String,

    /// Tick rate (Hz)
    #[arg(long, env = "WORLD_TICK_RATE_HZ", default_value_t = 30.0)]
    tick_rate_hz: f32,

    /// Terrain seed
    #[arg(long, env = "WORLD_SEED", default_value_t = 42)]
    seed: u64,

    /// Streaming cell size in world units
    #[arg(long, env = "WORLD_CELL_SIZE", default_value_t = 10.0)]
    cell_size: f32,

    /// Streaming activation radius (Chebyshev, in cells)
    #[arg(long, env = "WORLD_ACTIVATION_RADIUS", default_value_t = 16)]
    activation_radius: i32,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("janet_world=debug".parse()?),
        )
        .init();

    let args = Args::parse();

    log::info!(
        "Starting janet-world-server (session='{}', seed={}, cell_size={}, radius={})",
        args.session,
        args.seed,
        args.cell_size,
        args.activation_radius,
    );

    // Build world data layer
    let terrain = Arc::new(HeightmapTerrain::new(
        args.seed,
        // Use chunk_size = cell_size * activation_radius for sensible terrain chunks
        args.cell_size * 4.0,
        64, // base resolution at LOD 0
    ));
    let world = Arc::new(World::new(terrain));

    // Physics registry (standalone – no coordinator owning it)
    let physics_registry = Arc::new(RwLock::new({
        let mut reg = PhysicsRegistry::new(PhysicsRegistryConfig::default());
        let metadata = SimulationMetadata {
            id: "world-default".to_string(),
            mandate_id: "_world_default".to_string(),
            ontology: OntologyId::Custom {
                id: "Rapier2D".to_string(),
            },
            tier: Tier::Decidable,
            overlays: vec![],
            simulation_type: SimulationType::Rapier2D,
            created_at_frame: 0,
            name: "World Physics".to_string(),
            description: Some("janet-world-server default physics simulation".to_string()),
            generator_id: None,
        };
        let sim = Rapier2DSimulation::new(metadata, Rapier2DConfig::default());
        reg.set_default_simulation(Box::new(sim));
        reg
    }));

    // World service config
    let service_config = WorldServiceConfig {
        cell_size: args.cell_size,
        activation_radius: args.activation_radius,
        world_seed: args.seed,
        physics_dt: 1.0 / args.tick_rate_hz,
        ..Default::default()
    };

    let service = Arc::new(parking_lot::Mutex::new(WorldService::new(
        service_config,
        physics_registry,
        world,
    )));

    // Bus agent config
    let bus_config = WorldBusConfig {
        session: args.session,
        participant_id: args.participant_id,
        endpoint: args.endpoint,
        tick_rate_hz: args.tick_rate_hz,
    };

    // Run until shutdown
    WorldBusAgent::new(bus_config, service).run().await
}
