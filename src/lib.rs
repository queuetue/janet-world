//! Janet World Engine
//!
//! A 3D isometric world engine running as a standalone janet bus participant.
//!
//! ## Architecture
//!
//! ```text
//! WorldBusAgent  (bus.rs)
//!   └── WorldService  (service.rs)  ← streaming, cell lifecycle
//!         └── World  (structure.rs) ← data layer
//!               ├── HeightmapTerrain  (terrain.rs)
//!               └── StructureRegistry (structure.rs)
//! ```
//!
//! `WorldService` drives physics via `janet-operations::PhysicsRegistry`.
//! `WorldBusAgent` connects to the janet bus as an *external physics*
//! participant (role = `world`, capability `external_physics = true`).

// Protocol types are always available (no server feature needed).
pub mod protocol;
pub mod types;

// Server-side modules require the `server` feature.
#[cfg(feature = "server")]
pub mod bus;
#[cfg(feature = "server")]
pub mod service;
#[cfg(feature = "server")]
pub mod structure;
#[cfg(feature = "server")]
pub mod terrain;

// Convenience re-exports (server only)
#[cfg(feature = "server")]
pub use bus::{WorldBusAgent, WorldBusConfig};
#[cfg(feature = "server")]
pub use service::WorldService;
#[cfg(feature = "server")]
pub use structure::{StructureInstance, StructureRegistry, World};
#[cfg(feature = "server")]
pub use terrain::{HeightChunk, HeightmapTerrain, TerrainSource};
pub use types::{CellCoord, Vec3, WorldObject, WorldServiceConfig, WorldStats};
