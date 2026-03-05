# Janet World Expansion

This document tracks **remaining janet-world-side work** after moop integration
phases A→E. It is intentionally implementation-oriented so future effort can
start from this file directly.

## 1) WorldServiceConfig: add `tile_size_m`

- [x] Add `tile_size_m: f32` to `WorldServiceConfig` with default `2.0`.

### Why

Moop and Python terrain generation assume 2m tiles. The world service should
advertise tile resolution explicitly so clients do not infer scale implicitly.

### Likely files

- `src/service.rs` (`WorldServiceConfig`, constructors/defaults, any config parsing)
- `src/protocol.rs` (if config snapshots/events include service config)
- `src/bin/*` or startup wiring (if config is built from CLI/env)

### Implementation notes

1. Add field to config struct:
    - `pub tile_size_m: f32`
    - default value: `2.0`
2. Ensure validation (if present) rejects non-positive values.
3. Ensure runtime components reading chunk/tile size consume this field where
    appropriate (without changing existing chunk-size semantics unless intended).
4. If world-info or snapshot payload includes scale metadata, include
    `tile_size_m` there.

### Acceptance criteria

- Service starts without explicit tile-size config and uses `2.0`.
- Explicit config override changes the runtime value.
- Existing integration tests continue to pass.

### Tests

- Unit test for config default.
- Unit/integration test for config override path.

---

## 2) `ChunkActivated`: protocol expansion

- [x] Ensure `ChunkActivated` carries `seed`, `tile_resolution`, `lod`,
        `terrain_algo_version` (and keeps `terrain_seed` compatibility path as needed).

### Why

Clients need deterministic terrain reconstruction and algorithm-version
safety to prevent silent visual divergence across Rust/Python implementations.

### Likely files

- `src/protocol.rs` (`ChunkActivated` struct and serde)
- `src/service.rs` (event construction in activation/snapshot paths)
- `../plantangenet/world/protocol.py` (Python mirror + `from_dict` compatibility)
- `tests/*` in both Rust and Python protocol/roundtrip suites

### Implementation notes

1. Keep wire compatibility strategy explicit:
    - new writers emit all fields;
    - older payloads still parse where practical.
2. `tile_resolution` should represent tile size/resolution unambiguously
    (document units in protocol comments).
3. `terrain_algo_version` should be a stable version string or integer constant
    owned by the terrain implementation.
4. Ensure snapshot and live chunk-activated emissions use the same field set.

### Acceptance criteria

- `ChunkActivated` events contain all required fields in live and snapshot paths.
- Python protocol mirror parses new and legacy payload shapes.
- Terrain alignment tests remain green.

### Tests

- Rust serialization/deserialization roundtrip for expanded payload.
- Integration test asserting emitted chunk events include all fields.
- Python `from_dict` compatibility tests.

---

## 3) Handle `action.move` in janet-world

- [x] Implement `action.move` handling that applies velocity/motion to the
        participant physics body and emits resulting fiction/reality signals.

### Why

Moop now emits connected-mode move intents via `WorldClient`; janet-world
must execute approved `action.move` commands to complete the authoritative loop.

### Likely files

- `src/bus.rs` (or bus agent command dispatch)
- `src/service.rs` (movement application + participant/body lookup)
- `src/world.rs` / physics integration modules (velocity/body mutation)
- `src/protocol.rs` (action payload structs if missing)

### Implementation notes

1. Parse validated `action.move` payload from coordinator.
2. Resolve target participant/entity to a physics body.
3. Apply movement as velocity or impulse according to existing physics model.
4. Emit/update transform fiction event (`fiction.entity.transform`) in normal
    frame flow (avoid ad-hoc out-of-band state mutation).
5. Ensure collisions/terrain constraints remain enforced by physics step.

### Acceptance criteria

- Approved move actions produce authoritative position change over frames.
- Transform events are emitted with updated position/velocity.
- Invalid participant/body mapping returns clear error without panic.

### Tests

- Command-handler unit test for valid move action.
- Integration test: action.move -> transform event emitted.
- Negative test: unknown participant/entity id.

---

## 4) Handle `action.interact` in janet-world

- [ ] Implement `action.interact` handling for resource depletion + emitted
        fiction updates.

### Why

Moop-side collect/interact wiring exists; janet-world must own authoritative
resource mutations and publish outcomes for coordinator arbitration.

### Likely files

- `src/bus.rs` (action dispatch)
- `src/service.rs` (interaction logic entrypoint)
- Resource/state modules (chunk resource values, depletion rules)
- `src/protocol.rs` (interaction outcome events if not present)

### Implementation notes

1. Parse approved interaction action and validate target context
    (chunk/entity/resource node).
2. Apply depletion/mutation atomically in world state.
3. Emit fiction update events describing the new resource state.
4. Keep idempotency/replay behavior explicit (important for retries).
5. Define clear failure semantics: out-of-range, depleted, invalid target,
    unauthorized verb, etc.

### Acceptance criteria

- Successful interact mutates resource state and emits update event(s).
- Repeated interact on depleted target returns deterministic failure/zero effect.
- Downstream moop observation reflects updated resource values after arbitration.

### Tests

- Unit tests for depletion rules and edge cases.
- Integration test: action.interact -> fiction update -> observer-visible change.
- Regression test for non-existent target handling.

---

## Execution Order (recommended)

1. `ChunkActivated` expansion (protocol safety + metadata completeness).
2. `tile_size_m` config field (scale explicitness).
3. `action.move` execution path.
4. `action.interact` execution path.

Rationale: protocol/config first reduces rework while wiring runtime behavior.
