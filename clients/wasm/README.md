# Janet World — WebAssembly Client

> **Status: planned** — this client is not yet implemented.  This document
> describes the intended design and API surface to guide contributors.

A Rust → WebAssembly module (via [`wasm-bindgen`](https://rustwasm.github.io/wasm-bindgen/))
that connects a browser-based game to a running [janet-world](../../) server.

Because browsers cannot open raw TCP NATS connections, this client speaks to a
**NATS WebSocket proxy** ([`nats.ws`](https://github.com/nats-io/nats.ws) or a
simple `nats-server --websocket` endpoint).  Everything above the transport
layer uses the same wire protocol as all other janet-world clients.

---

## Planned architecture

```
Browser (main thread)
───────────────────────────────────────────
Your game loop
  └─ JanetWorldClient (JS class, wasm-bindgen)
       │  poll()          ← drains event queue each frame
       │  sendMovement()  → queues intent for NATS publish
       │
wasm module (Rust, compiled to wasm32-unknown-unknown)
  └─ bridge task (wasm-bindgen-futures / spawn_local)
       web_sys::WebSocket → nats-ws framing
       events → crossbeam-style ring buffer → JS callbacks
```

The wasm bridge runs inside the browser's single thread using async
`spawn_local` (no web workers required, though a worker port is a future
optimisation).  The Godot/Unreal client's threading model is replaced by
coop-scheduled futures.

---

## Quick start (once implemented)

### 1. Install prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
# Start NATS with WebSocket support:
nats-server --websocket --wsport 9222
```

### 2. Build

```bash
cd clients/wasm
wasm-pack build --target web --release
# Output: pkg/janet_world_wasm.js + janet_world_wasm_bg.wasm
```

### 3. Use in a browser game

```js
import init, { JanetWorldClient } from './pkg/janet_world_wasm.js';

await init();

const world = new JanetWorldClient({
  endpoint: 'wss://nats01.internal.plantange.net/',   // NATS WebSocket proxy
  session: 'default',
  participantId: 'browser-client',
});

world.onChunkActivated((chunkId, cx, cy, seed, lod, chunkSize) => {
  spawnTerrainMesh(cx, cy, seed, lod, chunkSize);
});

world.onEntityTransform((entityId, x, y, z, rotY, vx, vy, vz, frame, dt) => {
  moveEntity(entityId, { x, y, z });
});

await world.connect();

// In your render loop:
function tick() {
  world.poll();               // flush event queue → fires callbacks
  requestAnimationFrame(tick);
}
tick();
```

---

## Planned API

### Constructor options

| Option | Default | Description |
|---|---|---|
| `endpoint` | `wss://nats01.internal.plantange.net/` | NATS WebSocket proxy URL |
| `session` | `default` | Janet session name |
| `participantId` | `wasm-client` | Identity on the bus |
| `eventBuffer` | `1024` | Max queued events before back-pressure |

### Event callbacks

| Method | Callback args | Fired when |
|---|---|---|
| `onConnectionState(cb)` | `state: string` | State changes: `connecting`, `active`, `disconnected`, `error` |
| `onChunkActivated(cb)` | `chunkId, cx, cy, seed, lod, chunkSize` | Terrain chunk activated |
| `onChunkDeactivated(cb)` | `chunkId` | Terrain chunk freed |
| `onStructureSpawned(cb)` | `structureId, typeId, x, y, z, rotY` | Structure entered active region |
| `onStructureRemoved(cb)` | `structureId` | Structure left active region |
| `onEntitySpawned(cb)` | `entityId, archetype, x, y, z, rotY` | Entity entered active region |
| `onEntityRemoved(cb)` | `entityId` | Entity left active region |
| `onEntityTransform(cb)` | `entityId, x, y, z, rotY, vx, vy, vz, frame, dt` | Authoritative transform tick |
| `onSnapshotBegin(cb)` | `frame` | Full snapshot started |
| `onSnapshotEnd(cb)` | *(none)* | Full snapshot complete |

### Methods

```ts
await world.connect()
world.disconnect()
world.poll()                                     // call each frame

world.sendMovement(dx: number, dy: number, dz: number)
world.sendInteraction(targetId: string, verb?: string)
world.teleport(x: number, y: number, z: number)
world.updateViewRadius(radius: number)
world.requestSnapshot(x: number, y: number, z: number, radius: number)

world.activeChunkCount(): number
world.entityCount(): number
world.isConnected(): boolean
world.lastFrame(): bigint
world.extrapolateEntity(entityId: string, elapsedSec: number): [x, y, z]
```

---

## Terrain design

Same as all other clients: the server sends only `(cx, cy, seed, lod,
chunkSize)` — **no height data is ever sent over the wire**.  The browser
generates the mesh locally from the seed using the same deterministic noise
function as the server.

---

## Contributing

If you want to implement this client:

1. The wire protocol is fully defined in [`../../src/protocol.rs`](../../src/protocol.rs)
   and the subject constants in `protocol::subjects`.
2. The Godot client in `../godot/src/` is the reference implementation —
   `bridge.rs`, `cache.rs`, and `events.rs` can be ported almost directly.
3. Use `wasm-bindgen-futures::spawn_local` in place of `tokio::spawn`.
4. Use `web-sys::WebSocket` (or the `nats.ws` npm package via `wasm-bindgen`) for transport.

---

## Requirements (planned)

| Dependency | Version |
|---|---|
| Rust | 1.75+ |
| wasm-pack | 0.12+ |
| nats-server with `--websocket` | 2.10+ |
| Browsers | Chrome 90+, Firefox 88+, Safari 15+ |

---

## License

MIT — see the root `Cargo.toml` for details.
