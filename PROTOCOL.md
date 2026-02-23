# Integration with Existing External Physics Protocol

We already have a structured external physics protocol operating over the control bus.

This is an asset. We should extend it rather than inventing a parallel world-streaming channel.

## Observations About Current Protocol

The protocol already supports:

* Deterministic frame stepping (`physics.step` / `step_response`)
* Snapshot + restore (state migration ready)
* Query channel (point-in-time inspection)
* Census sampling (selective state projection)
* Health + lifecycle management

This means the simulation core is already architected as a service boundary.

That is exactly what we need.

---

## Suitability Analysis

### 1. Frame Authority Model

The coordinator drives frames.
The executor responds with fiction updates.

This maps cleanly to:

WorldService + Terrain + Structures living inside the executor.

The Godot client should NOT step physics directly.
It should consume post-step deltas derived from `fiction_updates`.

Verdict: ✔ Suitable.

---

### 2. Census as Replication Filter

`CensusSampleConfig` allows selecting cohorts and keys.

This is extremely powerful for client streaming.

Instead of inventing:

* TerrainChunkActivated messages
* EntityTransformUpdated messages

We can treat streamed state as a *view-specific census projection*.

Client streaming becomes:

ClientPositionUpdate → Adjust census config → Coordinator samples → physics.step → fiction_updates → Publish to client channel.

Verdict: ✔ Very strong foundation.

---

### 3. Snapshot / Restore

Already present.

This gives us:

* Seamless client reconnect
* Region transfer
* Migration across servers
* Replay / rewind capability

This is far ahead of most engines.

Verdict: ✔ Production-grade primitive.

---

## Required Extensions for World Streaming

The physics protocol is step-oriented.
World streaming needs topology-oriented messages.

We should add a new domain namespace, not overload physics.

Recommended new action prefix:

`world.*`

Examples:

* world.chunk_activate
* world.chunk_deactivate
* world.structure_spawn
* world.structure_remove
* world.entity_spawn
* world.entity_remove

These messages are *derived from fiction_updates*, not replacements.

Physics remains authoritative; world messages are semantic projections.

---

## Recommended Architecture Adjustment

Coordinator Responsibilities:

1. Drive `physics.step`
2. Receive `fiction_updates`
3. Derive world deltas
4. Publish client-facing `world.*` messages

Executor Responsibilities:

* Maintain simulation state
* Produce fiction updates only

Godot Client Responsibilities:

* Subscribe to world.* channel
* Subscribe to entity transform deltas
* Send intent messages only

No client should ever speak raw physics protocol.

---

## Integration Layer Shape

Control Bus
├── physics.*(internal authority channel)
├── world.* (client projection channel)
└── intent.* (client → coordinator)

This keeps authority clean and layered.

---

## Risk Areas

1. Census Overreach
   If census samples become too large, bandwidth explodes.
   Mitigation: spatially bounded cohorts + delta compression.

2. Frame Drift
   If client interpolation assumes fixed frame rate, but step jitter occurs.
   Mitigation: include frame + dt in world delta messages.

3. Terrain Payload Size
   Heightfields are heavy.
   Mitigation:

* Deterministic terrain generation on client
* Only send seed + chunk coord + LOD

Never stream raw height arrays unless debugging.

---

## Final Verdict

Your existing external physics protocol is not just suitable.
It is structurally aligned with a distributed world engine.

We do not need to redesign the bus.
We need to layer semantic projection on top of it.

The integration effort is evolutionary, not architectural surgery.
