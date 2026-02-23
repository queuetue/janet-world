# Janet World — Babylon.js Client

> **Status: planned** — this client is not yet implemented.  This document
> describes the intended design and API surface to guide contributors.

A TypeScript package that connects a [Babylon.js](https://www.babylonjs.com)
scene to a running [janet-world](../../) server.

Builds on the same [`nats.ws`](https://github.com/nats-io/nats.ws) transport
as the Three.js client, but adds first-class Babylon.js helpers:
`JanetWorldObservable` wrappers (native to Babylon's event model),
`GroundMesh` chunk management, and a `TransformNode`-based entity tracker
with built-in dead-reckoning.

---

## Planned architecture

```
Browser (Babylon.js render loop)
──────────────────────────────────────────
JanetWorldClient (Babylon Observable pattern)
  connect()  → nats.ws WebSocket
  scene.registerBeforeRender(() => world.poll())
              → flushes event buffer before each frame
              → fires onChunkActivatedObservable, etc.

  sendMovement(Vector3)  → publish intent.move

nats.ws WebSocket
  ↕  ws://nats-server:9222
     subscribe world.*
     publish   intent.*
```

Events follow Babylon's native `Observable<T>` pattern, so they integrate
naturally with the Babylon Inspector and GUI components.

---

## Quick start (once implemented)

### 1. Install

```bash
npm install @janet/world-babylonjs
# or
pnpm add @janet/world-babylonjs
```

### 2. Start NATS with WebSocket support

```bash
nats-server --websocket --wsport 9222
```

### 3. Wire into your Babylon scene

```ts
import { Engine, Scene, Vector3 } from '@babylonjs/core';
import { JanetWorldClient } from '@janet/world-babylonjs';

const engine = new Engine(canvas, true);
const scene  = new Scene(engine);

const world = new JanetWorldClient(scene, {
  endpoint: 'wss://nats01.internal.plantange.net/',
  session: 'default',
  participantId: 'babylon-client',
});

// --- Terrain --- //
world.onChunkActivatedObservable.add(({ chunkId, cx, cy, seed, lod, chunkSize }) => {
  // Build a GroundMesh from the seed — no heightmap is transferred.
  const ground = buildTerrainGround(scene, cx, cy, seed, lod, chunkSize);
  chunks.set(chunkId, ground);
});

world.onChunkDeactivatedObservable.add(({ chunkId }) => {
  chunks.get(chunkId)?.dispose();
  chunks.delete(chunkId);
});

// --- Entities --- //
world.onEntitySpawnedObservable.add(({ entityId, archetype, x, y, z }) => {
  const mesh = spawnArchetype(scene, archetype);
  mesh.position.set(x, y, z);
  entities.set(entityId, mesh);
});

world.onEntityTransformObservable.add(({ entityId, x, y, z, rotY, vx, vy, vz, dt }) => {
  const mesh = entities.get(entityId);
  if (mesh) mesh.position.set(x, y, z);
});

world.onEntityRemovedObservable.add(({ entityId }) => {
  entities.get(entityId)?.dispose();
  entities.delete(entityId);
});

// --- Connect and run --- //
await world.connect();

engine.runRenderLoop(() => {
  scene.render();  // world.poll() is registered via scene.registerBeforeRender
});
```

---

## Planned API

### Constructor

```ts
new JanetWorldClient(scene: Scene, options: JanetWorldOptions)
```

| Option | Default | Description |
|---|---|---|
| `endpoint` | `wss://nats01.internal.plantange.net/` | NATS WebSocket proxy URL |
| `session` | `default` | Janet session name |
| `participantId` | `babylon-client` | Identity on the bus |
| `autoRegisterBeforeRender` | `true` | Auto-call `poll()` via `scene.registerBeforeRender` |
| `eventBuffer` | `2048` | Max buffered events between frames |

### Observables

| Observable | EventData fields | Fired when |
|---|---|---|
| `onConnectionStateObservable` | `state: string` | `connecting` / `active` / `disconnected` / `error` |
| `onChunkActivatedObservable` | `chunkId, cx, cy, seed, lod, chunkSize` | Terrain chunk activated |
| `onChunkDeactivatedObservable` | `chunkId` | Terrain chunk freed |
| `onStructureSpawnedObservable` | `structureId, typeId, position: Vector3, rotY, metadata` | Structure appeared |
| `onStructureRemovedObservable` | `structureId` | Structure removed |
| `onEntitySpawnedObservable` | `entityId, archetype, position: Vector3, rotY, metadata` | Entity appeared |
| `onEntityRemovedObservable` | `entityId` | Entity disappeared |
| `onEntityTransformObservable` | `entityId, position: Vector3, rotY, velocity: Vector3, frame, dt` | Authoritative transform |
| `onSnapshotBeginObservable` | `frame: bigint` | Snapshot started |
| `onSnapshotEndObservable` | *(none)* | Snapshot complete |

### Methods

```ts
await world.connect(): Promise<void>
world.disconnect(): void
world.poll(): void                              // manual call if autoRegisterBeforeRender = false

world.sendMovement(direction: Vector3): void
world.sendInteraction(targetId: string, verb?: string): void
world.teleport(position: Vector3): void
world.updateViewRadius(radius: number): void
world.requestSnapshot(position: Vector3, radius: number): void

world.activeChunkCount(): number
world.entityCount(): number
world.isConnected(): boolean
world.lastFrame(): bigint
world.extrapolateEntity(entityId: string, elapsedSec: number): Vector3 | null
```

### Built-in entity tracker (`autoTrack` option)

When `autoTrack: true`, the client maintains a `Map<string, TransformNode>`
where each node is automatically positioned from incoming `entity:transform`
events.  Dead-reckoning (linear extrapolation using the velocity payload) is
applied each frame before rendering.

```ts
const world = new JanetWorldClient(scene, { autoTrack: true });
// …
const node = world.getTrackedNode(entityId);
if (node) console.log(node.position);
```

---

## Terrain design

Only `(cx, cy, seed, lod, chunkSize)` is sent over the wire.  Babylon.js
projects can use `GroundMesh.CreateFromHeightMap` with a procedurally
generated canvas-based heightmap (from the seed) or a custom
`VertexData`-based approach for full LOD control.

---

## Contributing

1. Wire protocol: [`../../src/protocol.rs`](../../src/protocol.rs).
2. Reference implementation: `../godot/src/` for event handling logic;
   `../threejs/` for the JavaScript transport layer.
3. Use Babylon's `Observable<T>` rather than Node `EventEmitter` — this
   enables Babylon Inspector introspection of events.
4. The `autoTrack` feature would be a low-priority nice-to-have but
   adds real value for beginners.

---

## Requirements (planned)

| Dependency | Version |
|---|---|
| Node.js (build) | 20+ |
| nats.ws | 2.x |
| Babylon.js (peer) | 6.x+ |
| nats-server `--websocket` | 2.10+ |

---

## License

MIT — see the root `Cargo.toml` for details.
