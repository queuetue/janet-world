# 3D Isometric World Engine – Roadmap to Completion

## Current State (Where We Are)

The engine now supports:

* True 3D world coordinates (Vec3)
* Chunk-based spatial activation
* Deterministic heightmap terrain
* Multi-LOD terrain generation
* Heightfield colliders per chunk
* Physics-backed streaming activation/deactivation
* Clean separation between World (data) and WorldService (streaming)

At this stage, the terrain is physically real, chunked, and streamed deterministically around participants.

The world is no longer decorative — it is physically grounded.

---

# Phase 0 – Crate Infrastructure & Bus Integration

The world service is promoted from an embedded module inside the coordinator to
a first-class crate with its own binary.  It joins the janet bus as an
**external physics** participant, the same way the coordinator provisions itself.

## 0. Module Refactor

Split the monolithic `mod.rs` into focused modules:

```
janet-world/
  src/
    lib.rs          ← crate root, re-exports
    types.rs        ← Vec3, CellCoord, WorldObject, WorldStats, WorldServiceConfig
    terrain.rs      ← TerrainSource trait, HeightmapTerrain, HeightChunk
    structure.rs    ← StructureInstance, StructureRegistry, World
    service.rs      ← WorldService (tick, activate/deactivate, streaming)
    bus.rs          ← WorldBusAgent (janet-client integration, external physics)
  src/bin/
    world.rs        ← standalone binary entrypoint
  tests/
    terrain_tests.rs
    service_tests.rs
    bus_tests.rs
```

Outcome: each module is independently testable and does not grow unbounded.

## 1. Standalone Binary

`janet-world-server` binary:

* Reads config from env / TOML (session, NATS endpoint, seed, activation radius)
* Provisions itself as a `janet-client` participant with role `world`
* Announces capability `external_physics: true`
* Runs the physics + world tick loop inside a Tokio runtime

Outcome: the world engine can be deployed independently, as a sidecar, or as a
k8s pod alongside a coordinator.

## 2. Bus Integration (`bus.rs`)

`WorldBusAgent` wraps a `JanetExecutor` and drives the `WorldService`:

* Connects via `ParticipantBusFSM` (Hello → Active lifecycle)
* Accepts `participant_joined` / `participant_left` events to register/unregister
  streaming participants
* Emits `world_state_update` events on each terrain activation/deactivation
* Responds to `teleport`, `set_seed`, `reload_chunk` commands
* Exposes `world_stats` on demand

Outcome: the coordinator no longer owns world state; it delegates to the world
service over the bus.

## 3. Test Coverage

Unit tests co-located with each module:

* `terrain_tests` – noise correctness, chunk cache, LOD collider shape
* `service_tests` – cell activation/deactivation, determinism across seeds
* `bus_tests` – FSM state transitions, command dispatch, event emission

Integration tests in `tests/` exercise the full binary against a live bus.

---

# Phase 1 – Terrain Maturity

## 1. Proper Fractal Noise

Replace placeholder sine noise with:

* Fractal Brownian Motion (fBm)
* Multiple octaves
* Adjustable roughness
* Biome masks (future-ready)

Outcome:
Terrain becomes natural and tunable.

## 2. Bilinear Height Sampling

Upgrade `height_at()` to bilinear interpolation instead of nearest sample.

Outcome:
Smooth character grounding and slope stability.

## 3. Terrain Normals from Gradient

Use analytical gradient instead of finite epsilon sampling.

Outcome:
Stable lighting and slope-aware movement.

---

# Phase 2 – Physics Integration

## 1. Proper Character Controller

Implement capsule collider + slope rules:

* Max walk angle
* Step height
* Ground snapping
* Airborne state

Outcome:
DayZ-like grounded movement.

## 2. Heightfield LOD Physics Strategy

Near chunks:

* Full resolution collider

Far chunks:

* Reduced LOD collider

Very far chunks:

* Optional simplified plane

Outcome:
Physics cost scales with proximity.

## 3. Terrain Chunk Eviction

Add memory budget:

* Remove cached height chunks outside streaming radius
* Keep LRU list

Outcome:
Bounded memory footprint.

---

# Phase 3 – Structure Mesh System

## 1. Structure Data Model

Add:

* Mesh reference
* Collision mesh
* Transform
* Optional nav modifiers

Structures must be treated as topology, not decoration.

## 2. Structure Streaming

Integrate with cell activation:

* Static buildings streamed per chunk
* Registered as mesh colliders

Outcome:
Vertical combat and interiors become real.

## 3. Interior Support

Allow multi-floor structures:

* Vertical chunk activation
* Overlapping Z streaming

Outcome:
True 3D spaces, not fake layered tiles.

---

# Phase 4 – Navigation System

## 1. Per-Chunk Navmesh Baking

Bake navmesh from:

* Heightfield
* Static structures

Store navmesh per cell.

## 2. Dynamic Obstacle Injection

Allow runtime obstacles:

* Vehicles
* Temporary barriers

Outcome:
AI behaves spatially aware.

---

# Phase 5 – Rendering Pipeline

## 1. Isometric Projection Layer

Simulation remains true 3D.
Rendering projects via:

```
screen_x = x - y
screen_y = (x + y) * 0.5 - z
```

## 2. Depth Sorting Strategy

Sort by:

* z first
* projected y second

Handle occlusion volumes for buildings.

## 3. Terrain Mesh Generation

Convert height chunks into render meshes:

* GPU vertex buffers
* LOD per distance

Outcome:
Efficient terrain rendering.

---

# Phase 6 – World Streaming Finalization

## 1. Unified Streaming Layer

Single streaming authority controls:

* Terrain
* Structures
* Navmesh
* Render meshes
* Physics bodies

Activation radius becomes policy-driven.

## 2. Multiplayer Determinism

Ensure:

* Chunk activation is deterministic
* Terrain generation is seed-stable
* Structure placement is reproducible

Outcome:
Network-safe world replication.

---

# Phase 7 – Advanced Systems

* Biomes
* Weather interaction
* Destructible terrain (optional)
* Procedural structure placement
* Server-authoritative world diffs

---

# Final Architecture

```
janet-world-server (binary)
│
├── WorldBusAgent (bus.rs)
│     ├── JanetExecutor (janet-client)  ← bus participant, role=world
│     └── command/event dispatch
│
└── WorldService (service.rs)
      ├── World (structure.rs)
      │     ├── Terrain (terrain.rs)   ← HeightmapTerrain + LOD cache
      │     └── StructureRegistry
      ├── NavmeshChunks               (Phase 4)
      ├── RenderChunks                (Phase 5)
      └── PhysicsRegistry (janet-operations)
```

Physics and rendering become consumers of world state.  The coordinator
delegates spatial authority to the world service over the janet bus.

---

# Completion Criteria

The engine is "complete" when:

* `janet-world-server` boots, joins the bus, and becomes `Active` as an external physics participant
* Terrain streams seamlessly as participants move
* Structures stream and collide properly
* Characters move smoothly across slopes and stairs
* AI navigates height and interiors
* Rendering matches physics topology
* Memory and physics costs scale with player density
* All world state mutations are bus-auditable events

At that point, the isometric view is just a lens.

The world underneath is fully 3D and simulation-grade.
