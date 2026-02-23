# Janet World → Godot Integration Roadmap

This roadmap enforces a core principle:

Godot developers consume an engine API — not a message bus.

The Rust world service remains authoritative and bus-driven.
The Godot layer presents a native, idiomatic, scene-centric interface that hides distributed complexity.

---

# Phase 0 – Mental Model Alignment

Before building anything, lock in the contract:

• The Rust world service is authoritative.
• Godot is a viewport + input surface.
• The bus is infrastructure, not API.
• The integration layer translates distributed state into engine-native signals and nodes.

If a Godot developer sees transport details, the abstraction has failed.

---

# Phase 1 – Define the Godot-Facing API (Not the Bus API)

Instead of starting from message categories, define what a Godot developer expects to interact with.

## 1. Core Node

Create a top-level node:

`JanetWorldClient` (extends Node)

Responsibilities:

• Connect to bus via Rust GDExtension
• Maintain local world cache
• Emit high-level signals
• Expose intent methods

## 2. Signals (Primary Interface)

Signals are the public contract:

• chunk_activated(chunk_id, seed, lod)
• chunk_deactivated(chunk_id)
• structure_spawned(structure_id, transform, type_id)
• structure_removed(structure_id)
• entity_spawned(entity_id, archetype)
• entity_removed(entity_id)
• entity_transform(entity_id, transform, frame, dt)
• connection_state_changed(state)

These signals represent semantic world events derived from the bus.

No raw protocol structs leak into GDScript.

## 3. Intent Methods

Expose simple intent functions:

• send_movement(direction: Vector3)
• send_interaction(target_id)
• teleport(position: Vector3)
• update_view_radius(radius: float)

Internally these publish intent.* messages.

Externally they feel like ordinary method calls.

---

# Phase 2 – Rust GDExtension Bridge

The bridge is responsible for:

• Bus connection
• Serialization / deserialization
• Frame buffering
• Thread isolation
• Error handling

It exposes a minimal surface to Godot:

• start(config)
• stop()
• poll_events()
• send_intent(...)

All networking stays in Rust.

Godot receives processed semantic events only.

---

# Phase 3 – Client World Cache Layer

Inside the plugin, implement a cache that mirrors active world state:

ClientWorldCache
├── ActiveChunks
├── Structures
├── Entities
└── FrameBuffer

This layer:

• Deduplicates events
• Applies delta updates
• Maintains interpolation buffers
• Handles snapshot vs delta seamlessly

Snapshots hydrate the cache.
Deltas mutate it.

Godot code never distinguishes between them.

---

# Phase 4 – Terrain Strategy (Deterministic First)

Do not stream height arrays.

On chunk_activated:

1. Use seed + chunk coordinate
2. Generate terrain locally
3. Apply LOD
4. Insert MeshInstance3D

Optional:
• Lightweight visual collider

The server remains authoritative for physics.

This keeps bandwidth bounded and startup instant.

---

# Phase 5 – Entity Replication Model

Entities follow a buffered interpolation model.

Server sends authoritative transforms at fixed intervals.
Client:

• Stores last N frames
• Interpolates at render rate
• Applies reconciliation if drift exceeds threshold

No snapping unless correction magnitude exceeds tolerance.

Godot scene nodes represent proxies only.

---

# Phase 6 – Streaming as Policy, Not Mechanic

Client provides hints:

• position
• view radius

Server computes activation set deterministically.

The Godot layer simply reacts to activation events.

No client-side streaming authority exists.

---

# Phase 7 – Editor Experience

To feel native:

• Provide a JanetWorldConfig Resource
• Provide a dock panel showing:
– connection status
– current frame
– active chunk count
– entity count

Enable drop-in workflow:

1. Enable plugin
2. Add JanetWorldClient to scene
3. Press Play
4. World streams

If it requires a README tutorial to boot, it’s too complex.

---

# Phase 8 – Performance Envelope

Simulation: 30 Hz (authoritative)
Network updates: 10–20 Hz
Client interpolation: render FPS

Controls:

• Quantized transforms
• Seed-based deterministic terrain
• LOD-aware chunk activation
• Delta-only updates post-connect

Bandwidth scales with density, not map size.

---

# Final Architecture (Godot Perspective)

JanetWorldClient (Node)
├── RustBridge (GDExtension)
├── WorldCache
├── TerrainRenderer
├── EntityManager
└── Interpolator

From a Godot developer’s view:

The world behaves like a live multiplayer scene node.

From the system’s view:

Godot is a projection terminal over an authoritative distributed simulation.

---

# Completion Criteria

Integration is complete when:

• A Godot scene streams terrain deterministically
• Structures instantiate correctly from IDs
• Entities move smoothly via buffered transforms
• Streaming adapts to movement automatically
• Disconnect/reconnect restores identical state
• No raw protocol details are exposed to GDScript

At that point, the bus disappears behind the API.

And that’s the real goal.
