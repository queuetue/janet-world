# Janet-World ↔ Moop Integration Plan

How the Rust world engine (`janet-world`) and the Python explorer client
(`moop`) meet via the Janet coordinator bus.

> **Architecture in one sentence:** The system is composed of hosted physics
> domains. Each domain proposes state transitions (*fiction*) according to its
> own laws. The coordinator arbitrates these proposals and publishes the
> accepted sequence of events as *reality*. Participants receive reality events
> and construct local perception in their own private chem.
>
> **The constitutional rule:** Neither the world nor participants can directly
> change reality. Only the coordinator can.
>
> **The full loop:** `intent.*` → `action.*` → `fiction.*` → `reality.*` → local chem

---

## The Two Systems Today

### janet-world (Rust)

janet-world is **one hosted physics domain** responsible for terrain and spatial
topology.  Other domains (e.g., Newtonian physics, commerce, narrative systems)
may operate alongside it and contribute fiction updates to the coordinator.

janet-world is available as a submodule in plantangenet.

| Component | Responsibility |
| --- | --- |
| `HeightmapTerrain` | Seed-deterministic heightfield generation, LOD cache |
| `StructureRegistry` | Static mesh instances (buildings, rocks, barriers) |
| `WorldService` | Streaming cell activation/deactivation around participants |
| `WorldBusAgent` | NATS integration — publishes `fiction.*` events, handles `action.*` commands |

Key design choices:

- Terrain is **never** sent as raw height arrays — only `(seed, cx, cy, lod, chunk_size)`.
- Positions are in **metres**.  `cell_size` (default 10 m) drives the streaming grid.
- janet-world does not directly change reality; it proposes fiction that the coordinator ratifies.

### moop (Python)

A human-directed expedition participant that bridges a Godot/browser client
and the NATS bus.  It builds a local `MoopState` from observations received
through `chem`, applies fog-of-war, and broadcasts chunk lifecycle messages
(`CHUNK_ENTER` / `CHUNK_UPDATE` / `CHUNK_EXIT`) to the client over WebSocket.

Key design choices:

- The client renders **state**, not truth.  The moop never proxies raw terrain.
- `MoopChunk` carries summary fields (terrain_type, elevation, resources,
  hazard) plus optional tile-detail grids loaded via `sense`.
- World scale: `TILE_SIZE_M=2m`, `CHUNK_SIZE_M=32m`, `MOVE_STEP_M=1m`.
- Simulation mode uses a local chem dict; connected mode reads from NATS.
- `world_info` message on WS connect advertises all scale constants.

### plantangenet.world (Python)

The canonical Python interface to the janet-world ecosystem.  All Python
agents interacting with janet-world should use this package.

| Module | Responsibility |
| --- | --- |
| `protocol.py` | Python dataclasses mirroring all Rust `protocol.rs` types (`ChunkActivated`, `IntentMove`, `SUBJECTS`, …) |
| `client.py` | `WorldClient` — wraps a mukalo executor; typed send/receive and callback registration |
| `terrain.py` | `WorldTerrainGenerator` — canonical deterministic terrain generator; same `(seed, cx, cy)` always produces identical tiles |

**Tile generation is always client-side.**  Both moop's simulation mode and
its connected-mode projection call `WorldTerrainGenerator(seed).generate_chunk(cx, cy)`
locally.  Tile arrays are never transmitted over the bus.  This keeps message
size minimal (4-scalar summary per chunk) and ensures every Python observer
produces identical terrain from the same inputs.

`moop.stable_test_world.WorldChunkGenerator` is a backward-compatible alias
for `WorldTerrainGenerator`.  New code should import from `plantangenet.world`
directly.

---

## The Coordinator in the Middle

The coordinator is the **constitutional arbiter**: it receives fiction from
hosted physics domains and intent from participants, enforces policy and
causal ordering, and publishes the accepted sequence of state changes as
reality events.  Neither the world nor participants can bypass it.

```text
hosted physics           coordinator              participant
(e.g. janet-world)         (arbiter)              (e.g. moop)
       │                       │                      │
       │── fiction.* ─────────►│                      │
       │                       │── reality.* ────────►│
       │                       │                      │
       │                       │                      │ (update local chem)
       │◄── action.* ──────────│◄── intent.* ─────────│
```

1. Participant desires flow upward as `intent.*` messages.
2. Coordinator authorizes and issues `action.*` commands to the appropriate
   hosted physics domain.
3. The physics domain computes the outcome and emits `fiction.*` proposals.
4. Coordinator validates and orders those proposals, publishing the accepted
   sequence as `reality.*` events.
5. Each participant executor receives `reality.*` events and updates its own
   **local chem** — its private perception model.

Chem is **executor-local**. The coordinator publishes reality events; it does
not write into any participant's chem. Participants derive their own perception
from the reality stream independently.

The moop never talks to janet-world directly. Reality events are the data plane.

---

## The Four-Stage Event Chain

Every state change in the system travels through four named stages:

| Stage | Subject pattern | Meaning | Publisher |
| --- | --- | --- | --- |
| Desire | `intent.*` | What a participant wants to do | participant |
| Authorization | `action.*` | What the coordinator approves | coordinator |
| Proposal | `fiction.*` | What the physics domain says happened | hosted physics domain |
| Acceptance | `reality.*` | What the coordinator ratifies as truth | coordinator |

Hosted physics domains emit `fiction.*` events describing the outcomes of
their simulation step.  The coordinator validates and orders these proposals,
publishing the accepted sequence as `reality.*` events visible to participants.
If you log all four stages you can deterministically reconstruct every
timeline decision.

### Bus Subject Conventions

| Subject | Example | Notes |
| --- | --- | --- |
| `intent.move` | participant → coordinator | Desire; may be rejected |
| `intent.teleport` | participant → coordinator | Desire; bounds-checked |
| `intent.interact` | participant → coordinator | Desire; policy-checked |
| `intent.sense` | participant → coordinator | Desire; rate-limited |
| `action.move` | coordinator → janet-world | Approved execution |
| `action.teleport` | coordinator → janet-world | Approved execution |
| `action.interact` | coordinator → janet-world | Approved execution |
| `fiction.chunk.activated` | janet-world → coordinator | Terrain proposal |
| `fiction.entity.transform` | physics domain → coordinator | Position proposal |
| `reality.chunk.activated` | coordinator → participants | Accepted terrain event |
| `reality.entity.transform` | coordinator → participants | Accepted position event |

---

## Data Model Mapping

### Scale Reconciliation

**World grid always dominates.**  Moop adopts the scale advertised by
janet-world, never the reverse.

| Concept | janet-world | moop | Resolution |
| --- | --- | --- | --- |
| Position unit | metres (f32) | metres (float) | **Already aligned** |
| Streaming cell | `cell_size` (default 10 m) | — | janet-world concern only |
| Chunk (client) | `ChunkActivated { cx, cy, chunk_size }` | `MoopChunk { chunk_id, tile_width, tile_height }` | See "Chunk Transcoding" |
| Tile | heightfield samples at `tile_size_m` resolution | `MoopTile` (2 m × 2 m) | Client re-derives from seed — tiles never transmitted |
| Move step | server resolves intent velocity | `MOVE_STEP_M = 1.0 m` | Physics-authoritative; moop's step is cosmetic in connected mode |

### Chunk Transcoding

janet-world streams **terrain activation events** containing
`(cx, cy, seed, lod, chunk_size)`.  It does **not** send height arrays or
tile grids — those are always regenerated locally.

moop streams **observation chunks** containing `(chunk_id, terrain_type, elevation,
resources_remaining, hazard_risk, center, bounds)`.  These are a semantic
projection of the chunk, not a sampling of its raw data.

The projection layer is a **local perception adapter**: it consumes
`reality.chunk.activated` events and derives chunk observations for the
executor's local chem.  It does not write to any shared store.

```text
reality.chunk.activated
        │
        ▼
projection adapter (executor-local)
        │
        ▼
local chem observation entry
```

The adapter does **two separate jobs** that must not be conflated:

#### Job 1 — Semantic Translation (projection adapter's only responsibility)

Receive `reality.chunk.activated`, regenerate the chunk locally, compute
the 4-scalar summary, and update the executor's local chem:

```python
from plantangenet.world import WorldTerrainGenerator, summarise_chunk

chunk = WorldTerrainGenerator(seed).generate_chunk(cx, cy)
summary = summarise_chunk(chunk)   # ChunkSummary — 4 scalars only

# executor-local chem only — not a shared store
self.chem["region/observation/{pid}"][chunk_id] = {
    **summary.as_dict(),           # terrain_type, elevation, resources, hazard
    "chunk_id": chunk_id,
    "last_observed_frame": frame,
    "scale": "local",
    "partition": f"{seed:08x}",
    "center": center_vec,
    "bounds": bounds_vecs,
}
```

No tile arrays are written here.  The adapter's output is always a fixed
4-scalar summary entry in the executor's own chem.

#### Job 2 — Tile Detail (triggered by `intent.sense`, resolved client-side in moop)

When the player issues a `sense` action, the moop regenerates the chunk
from the same seed and updates its local chem cache:

```python
# moop side — on sense action; updates local chem cache only
chunk = WorldTerrainGenerator(self._world_seed).generate_chunk(cx, cy)
tile_detail = _chunk_to_chem_tiles(chunk_id, chunk, frame)
self.chem["region/tiles/{pid}/{chunk_id}"] = tile_detail  # local cache
```

Tile detail format (matches `MoopChunk.load_tiles()`):

```json
{
  "width": 16,
  "height": 16,
  "tiles": [
    { "tile_id": "...", "terrain_type": "grass", "elevation": 0.48, ... },
    ...
  ]
}
```

Tiles are **never transmitted over the bus**.  The same seed + coordinates
always produce the same tile grid.  `ChunkStore.as_chem_tiles()` in
`moop.stable_test_world` is the reference implementation of this format.

### Chunk ID Format

janet-world uses `"{cx}:{cy}"`.  moop uses `"{scale}/{region_id}/{cx},{cy}"`.

Convention: the projection layer writes moop-format IDs:

```python
local/{seed_hex8}/{cx},{cy}
```

where `seed_hex8` is the lower 8 hex digits of the world seed.

### Entity → Squad Position Mapping

janet-world emits `fiction.entity.transform` events containing
`EntityTransform { entity_id, x, y, z, vx, vy, vz, dt }` for every tracked
participant each tick.  The coordinator ratifies these as `reality.entity.transform`
events.  Each participant executor ingests those reality events and updates
its own local chem:

```python
# executor-local — updated on reality.entity.transform
self.chem["region/participant/{pid}/position"] = { "x": f32, "y": f32 }
```

Moop reads this key in `on_frame()` and pushes it to `MoopState.own_position`
and `squad_positions`.

### Chunk Grid Alignment

janet-world's `cell_size` is configurable (default 10 m).  moop's `CHUNK_SIZE_M`
must follow whatever janet-world advertises — moop's chunk granularity never
drives the world cell size.

When `cell_size != CHUNK_SIZE_M`:

- `cell_size < CHUNK_SIZE_M`: aggregate N world cells into one moop chunk
  (plurality terrain type, mean elevation, max hazard).
- `cell_size > CHUNK_SIZE_M`: one world cell spans multiple moop chunks;
  synthesise sub-chunks from the same seed at finer granularity.

The advertised `cell_size` arrives via the `world_info` WS message or via a
coordinator config publication.

---

## Implementation Phases

### Phase A — Shared Scale Constants *(done)*

- [x] `TILE_SIZE_M = 2.0`, `CHUNK_SIZE_M = 32.0`, `MOVE_STEP_M = 1.0` defined in moop.
- [x] `world_info` message sent on WS connect.
- [x] MUD client uses advertised scale.
- [x] `plantangenet.world.terrain` is the canonical Python terrain generator.
- [ ] janet-world `WorldServiceConfig` gains `tile_size_m: f32` field (default 2.0).
- [ ] `ChunkActivated` gains `seed`, `tile_resolution`, `lod`, and `terrain_algo_version` fields.
  (`terrain_algo_version` prevents silent divergence when the noise algorithm changes.)

### Phase A.2 — Terrain Alignment Test Vectors *(before Phase B)*

Cross-language terrain determinism must be verified before the projection
layer is built.  A mismatch here means simulation mode and connected mode show
different worlds.

The Python side is now canonical: `plantangenet.world.terrain.WorldTerrainGenerator`.
The Rust side must produce byte-identical tile values for the same `(seed, cx, cy)`.

Deliverables:

- [ ] Pin canonical test vectors from the Python generator into
  `tests/terrain_alignment_vectors.json` (at least 5 seed/chunk pairs,
  spot-checking individual tiles).
- [ ] Implement MD5-seeded value noise in Rust (`janet-world/src/terrain.rs`)
  matching the Python algorithm exactly (same hash key format, same smoothstep,
  same octave weights).
- [ ] Integration test asserting tile-for-tile equality for all pinned vectors.
- [ ] Add `terrain_seed` field to `ChunkActivated` protocol message.

### Phase B — Chem Projection Layer

A new Python component — coordinator hook or standalone sidecar — subscribes
to `world.chunk.activated`, re-derives the chunk summary client-side using
`WorldTerrainGenerator`, and updates the executor’s chem observation entries.

**Responsibility boundary**: semantic translation only.  The projection layer
calls `WorldTerrainGenerator(seed).generate_chunk(cx, cy)` → `summarise_chunk()`
→ writes summary.  It does **not** write tile arrays (tile detail is
regenerated by the moop on `sense`).

Implementation sketch:

```python
from plantangenet.world import WorldClient, ChunkActivated
from plantangenet.world import WorldTerrainGenerator, summarise_chunk

class MoopProjection:
    def __init__(self, executor, seed: int):
        self._client = WorldClient(executor, config)
        self._gen = WorldTerrainGenerator(seed)
        self._client.on_chunk_activated(self._project_chunk)

    async def _project_chunk(self, event: ChunkActivated):
        chunk = self._gen.generate_chunk(event.cx, event.cy)
        summary = summarise_chunk(chunk)
        chunk_id = f"local/{event.seed:08x}/{event.cx},{event.cy}"
        # write summary to chem ...
```

Deliverables:

- [ ] `moop/projection.py` — `MoopProjection` subscribes to `world.chunk.activated`,
  writes chem summary key.
- [ ] Projection respects `activation_radius` — only writes chunks within
  the moop's perception radius.
- [ ] Tile detail written lazily by the moop on `sense`, not by the projection.
- [ ] Integration test: projection writes chem, moop reads it, WS client
  receives `CHUNK_ENTER` with correct summary.

### Phase C — Connected-Mode Intent Wiring

Wire moop's `send_command` to produce real bus `intent.*` messages routed
through the coordinator.

| moop publishes | Coordinator arbitrates | Coordinator issues | janet-world executes |
| --- | --- | --- | --- |
| `intent.move` | velocity, collision check | `action.move` | physics update |
| `intent.teleport` | bounds check | `action.teleport` | position set |
| `intent.sense` | rate limit | `action.sense` | observation reply |
| `intent.interact` | rules check | `action.interact` | resource depletion |

Deliverables:

- [ ] `ExplorerMoop.send_command` maps intent names to `SUBJECTS.*` constants
  from `plantangenet.world.protocol`.
- [ ] `WorldClient.move()` / `teleport()` / `interact()` used in connected
  mode (already exist in `plantangenet.world.client`).
- [ ] janet-world handles `action.move` — apply velocity to physics body.
- [ ] janet-world handles `action.interact` — resolve resource depletion,
  emit fiction update.

### Phase D — LOD + Fog of War Convergence

janet-world has LOD levels (0/1/2 by distance).  moop has fog-of-war
(confidence decay per frame without observation).

These should compose:

- **LOD** controls *geometry detail* — fewer terrain samples, coarser colliders.
- **Fog** controls *knowledge freshness* — confidence decays, resources may have
  changed.

In practice:

- Distant chunks arrive at LOD 1–2 with a single summary observation.
- Nearby chunks arrive at LOD 0 with full tile detail available on `sense`.
- Re-observation refreshes confidence; the projection layer re-runs the summary.

Deliverables:

- [ ] `ChunkActivated` includes `lod` field.
- [ ] Projection layer includes `lod` in its chem observation.
- [ ] `MoopChunk` gains `lod: int = 0` field.
- [ ] `MoopState.decay_confidence` accounts for LOD — distant chunks decay slower.

### Phase E — Entity & Structure Streaming

Beyond terrain, janet-world streams structures and entities.  These need
their own chem keys.

```text
region/structures/{pid}     →  { structure_id: { type_id, x, y, z, ... } }
region/entities/{pid}       →  { entity_id: { archetype, x, y, z, ... } }
```

Moop's `MoopState` grows corresponding dicts; `ws_protocol` gains
`STRUCTURE_ENTER` / `ENTITY_ENTER` messages (or they ride as metadata on
`CHUNK_ENTER`).

This is Phase 3+ work — terrain integration comes first.

---

## Python Interface Summary

All Python code interacting with the janet-world ecosystem imports from
`plantangenet.world`:

```python
# Terrain — canonical, client-side, deterministic
from plantangenet.world import WorldTerrainGenerator, summarise_chunk
from plantangenet.world import Tile, Chunk, ChunkSummary, TERRAIN_TYPES

# Protocol types (mirrors of Rust protocol.rs)
from plantangenet.world import (
    ChunkActivated, EntityTransform, WorldSnapshot,
    IntentMove, IntentTeleport, IntentInteract,
    SUBJECTS,
)

# Bus client (wraps mukalo executor)
from plantangenet.world import WorldClient, WorldClientConfig
```

---

## Chem Key Namespace Reference

All chem entries are **executor-local**. The coordinator never writes into a
participant's chem. Entries are populated by the executor itself in response
to `reality.*` events received from the bus.

| Key pattern | Populated by | Source event | Content |
| --- | --- | --- | --- |
| `region/observation/{pid}` | projection adapter | `reality.chunk.activated` | `{ chunk_id: summary_dict }` |
| `region/tiles/{pid}/{chunk_id}` | moop (on `sense`) | `reality.chunk.activated` + local regen | `{ width, height, tiles: [...] }` |
| `region/participant/{pid}/position` | moop executor | `reality.entity.transform` | `{ "x": f, "y": f }` |
| `expedition/squad/{squad_id}/members` | moop executor | `reality.squad.*` | `[pid, ...]` |
| `expedition/phase` | moop executor | `reality.expedition.*` | `"exploring"` etc. |

Tile arrays are **never** transmitted via the bus.  They are always regenerated
locally from `WorldTerrainGenerator(seed).generate_chunk(cx, cy)`.

---

## What Works Today Without a Coordinator

In simulation mode (`coordinator_url="mock"`):

1. `_sim_set_position` / `_sim_seed_chunks` write directly into `self.chem`.
2. `WorldTerrainGenerator(seed).generate_chunk(cx, cy)` → `summarise_chunk()`
   produces the exact summary shape written to `region/observation/{pid}`.
3. `on_frame` reads those keys and builds `MoopState`.
4. The MUD client or Godot renders the result.

This is the **stable test world** path — same data shapes, no bus.  The code
paths are identical; only the chem source differs.

When a real coordinator is available:

- Replace `_sim_*` methods with the projection adapter (Phase B), which
  consumes `reality.*` events and updates local chem identically.
- Movement is physics-authoritative instead of position += step.
- The moop's rendering code does not change.

---

## Sequence: First Connected Session

Terrain activation:

 1. janet-world boots, joins bus as a hosted physics domain
 2. moop executor boots, joins bus as participant (genome-derived ID)
 3. coordinator sees both, sends action.participant.join to janet-world
 4. janet-world activates streaming cells around participant position
 5. janet-world emits fiction.chunk.activated { cx, cy, seed, lod, terrain_algo_version }
 6. coordinator ratifies → publishes reality.chunk.activated
 7. moop projection adapter receives reality.chunk.activated:
      - WorldTerrainGenerator(seed).generate_chunk(cx, cy)
      - summarise_chunk() → 4-scalar summary
      - updates local chem["region/observation/{pid}"][chunk_id]
 8. moop.on_frame reads local chem, calls state.update_from_observation
 9. moop broadcasts CHUNK_ENTER to WS client
10. client renders terrain grid

Player movement:
11. client sends move (dx=1, dy=0) via WS
12. moop → WorldClient.move(1, 0, 0) → publishes intent.move on bus
13. coordinator arbitrates, issues action.move to janet-world
14. janet-world resolves movement (heightfield + colliders)
15. janet-world emits fiction.entity.transform { entity_id, x, y, z, ... }
16. coordinator ratifies → publishes reality.entity.transform
17. moop executor receives reality.entity.transform
18. moop updates local chem["region/participant/{pid}/position"]
19. moop.on_frame reads new position, broadcasts STATE_UPDATE
20. client interpolates to new position

Player sense:
21. client sends intent sense via WS
22. moop publishes intent.sense → coordinator arbitrates → action.sense
23. moop projection adapter regenerates full tile grid locally:
       chunk = WorldTerrainGenerator(seed).generate_chunk(cx, cy)
24. moop updates local chem cache: chem["region/tiles/{pid}/{chunk_id}"]
25. moop broadcasts CHUNK_UPDATE with tile detail to WS client

---

## Open Questions

1. **Who runs the projection layer?**  A Python sidecar on the coordinator is
   fastest to build and reuses `WorldTerrainGenerator`.  Long-term the
   coordinator may own it as a plugin.

   A: Python for now.

2. **3D → 2D position mapping.**  janet-world positions are Vec3 (x, y, z).
   moop uses (x, y).  For now `z = height_at(x, y)` is discarded on the moop
   side.  Godot will need it for mesh rendering — `MoopChunk.center` should
   carry the terrain z computed from the generator.

   A: The moop should become 3D.

3. **Resource model.**  moop's `resources_remaining` and `hazard_risk` are
   floats from the noise generator.  janet-world doesn't model resources yet.
   Phase B projection should supply initial values from `WorldTerrainGenerator`,
   and `action.interact` + collect should deplete them via fiction updates.

   A: I feel we should expand janet-world, or eliminate this in the moop.

4. **Terrain alignment across languages.**  *(Scheduled in Phase A.2 before
   Phase B.)*  The MD5-seeded value noise algorithm in
   `plantangenet.world.terrain` is the reference.  The Rust implementation
   must produce byte-identical tile values for the same `(seed, cx, cy)` triple.
   This is verified by the terrain alignment integration test.

   A: We should review using janet's entropy system for this, which is available via janet-abi.
