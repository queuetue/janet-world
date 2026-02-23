# Janet World — Three.js Client

> **Status: planned** — this client is not yet implemented.  This document
> describes the intended design and API surface to guide contributors.

A TypeScript/JavaScript package that connects a [Three.js](https://threejs.org)
scene to a running [janet-world](../../) server.

The client is a pure-JS library (no Rust/WASM required) that connects to NATS
through the official [`nats.ws`](https://github.com/nats-io/nats.ws) WebSocket
client.  It follows the same wire protocol as every other janet-world client,
exposing an `EventEmitter`-style API that maps directly onto Three.js scene
management patterns.

---

## Planned architecture

```
Browser (your Three.js game loop)
──────────────────────────────────────────
JanetWorldClient (EventEmitter)
  connect() → nats.ws WebSocket connection
  poll()    → flushes buffered events this frame
              emits 'chunk:activated', 'entity:transform', …

  sendMovement(dir: Vector3)
  →  client.publish('intent.move', JSON)

nats.ws WebSocket
  ↕  ws://nats-server:9222  (--websocket flag on nats-server)
     subscribe world.*
     publish   intent.*
```

All I/O is non-blocking (`async/await`).  Events are buffered in a JS array
and flushed synchronously inside your `requestAnimationFrame` loop via
`world.poll()`, keeping Three.js re-renders deterministic.

---

## Quick start (once implemented)

### 1. Install

```bash
npm install @janet/world-threejs
# or
pnpm add @janet/world-threejs
```

### 2. Start NATS with WebSocket support

```bash
nats-server --websocket --wsport 9222
```

### 3. Wire into your Three.js scene

```ts
import * as THREE from 'three';
import { JanetWorldClient } from '@janet/world-threejs';

const world = new JanetWorldClient({
  endpoint: 'wss://nats01.internal.plantange.net/',
  session: 'default',
  participantId: 'threejs-client',
});

// --- Terrain --- //
world.on('chunk:activated', ({ chunkId, cx, cy, seed, lod, chunkSize }) => {
  // Generate the mesh deterministically — no heightmap is sent over the wire.
  const mesh = buildTerrainMesh(cx, cy, seed, lod, chunkSize);
  scene.add(mesh);
  chunks.set(chunkId, mesh);
});

world.on('chunk:deactivated', ({ chunkId }) => {
  const mesh = chunks.get(chunkId);
  if (mesh) { scene.remove(mesh); chunks.delete(chunkId); }
});

// --- Entities --- //
world.on('entity:spawned', ({ entityId, archetype, x, y, z }) => {
  const obj = spawnEntity(archetype);
  obj.position.set(x, y, z);
  scene.add(obj);
  entities.set(entityId, obj);
});

world.on('entity:transform', ({ entityId, x, y, z, rotY, vx, vy, vz, dt }) => {
  const obj = entities.get(entityId);
  if (obj) obj.position.set(x, y, z);
});

world.on('entity:removed', ({ entityId }) => {
  const obj = entities.get(entityId);
  if (obj) { scene.remove(obj); entities.delete(entityId); }
});

// --- Connect and start --- //
await world.connect();

function animate() {
  requestAnimationFrame(animate);
  world.poll();      // flush event queue before rendering
  renderer.render(scene, camera);
}
animate();
```

---

## Planned API

### Constructor options

| Option | Default | Description |
|---|---|---|
| `endpoint` | `wss://nats01.internal.plantange.net/` | NATS WebSocket proxy URL |
| `session` | `default` | Janet session name |
| `participantId` | `threejs-client` | Identity on the bus |
| `eventBuffer` | `2048` | Max buffered events (excess dropped with warning) |

### Events (`world.on(event, handler)`)

| Event | Payload fields | Fired when |
|---|---|---|
| `connection:state` | `state: string` | `connecting` / `active` / `disconnected` / `error` |
| `chunk:activated` | `chunkId, cx, cy, seed, lod, chunkSize` | Terrain chunk activated |
| `chunk:deactivated` | `chunkId` | Terrain chunk freed |
| `structure:spawned` | `structureId, typeId, x, y, z, rotY, metadata` | Structure appeared |
| `structure:removed` | `structureId` | Structure removed |
| `entity:spawned` | `entityId, archetype, x, y, z, rotY, metadata` | Entity appeared |
| `entity:removed` | `entityId` | Entity disappeared |
| `entity:transform` | `entityId, x, y, z, rotY, vx, vy, vz, frame, dt` | Authoritative transform |
| `snapshot:begin` | `frame` | Snapshot started |
| `snapshot:end` | *(none)* | Snapshot complete |

### Methods

```ts
await world.connect(): Promise<void>
world.disconnect(): void
world.poll(): void                           // call once per animation frame

world.sendMovement(dir: THREE.Vector3): void
world.sendInteraction(targetId: string, verb?: string): void
world.teleport(pos: THREE.Vector3): void
world.updateViewRadius(radius: number): void
world.requestSnapshot(pos: THREE.Vector3, radius: number): void

world.activeChunkCount(): number
world.entityCount(): number
world.isConnected(): boolean
world.lastFrame(): bigint
world.extrapolateEntity(entityId: string, elapsedSec: number): THREE.Vector3 | null
```

---

## Terrain design

Only `(cx, cy, seed, lod, chunkSize)` is sent over the wire — **no
heightmap data**.  Your Three.js project generates the geometry locally
using any noise library that accepts the same seed.  The reference
implementation on the server uses simplex noise; a JS port
(`simplex-noise` npm package) produces identical results given the same seed.

---

## Contributing

1. Wire protocol: [`../../src/protocol.rs`](../../src/protocol.rs) and
   `protocol::subjects` constants.
2. Reference implementation: the Godot client in `../godot/src/` — especially
   `bridge.rs` (NATS subscribe loop) and `cache.rs` (state mirror).
3. Transport: use `nats.ws` (`npm install nats`) — connects to a standard
   `nats-server --websocket` endpoint, no proxy needed.
4. The package should have **zero runtime dependencies on Three.js** so it
   works equally well as a standalone NATS world client.

---

## Requirements (planned)

| Dependency | Version |
|---|---|
| Node.js (build) | 20+ |
| nats.ws | 2.x |
| Three.js (peer) | r150+ |
| nats-server `--websocket` | 2.10+ |

---

## License

MIT — see the root `Cargo.toml` for details.
