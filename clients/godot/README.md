# Janet World — Godot 4 GDExtension Client

A Godot 4 GDExtension that connects your game to a running
[janet-world](../../) server over the janet bus (NATS).

The client exposes a single drop-in `JanetWorldClient` node.  Your GDScript
connects to its signals and calls its methods; all NATS I/O and state
management is handled in a background Rust thread.

---

## Quick start

### 1. Build the library

```bash
cd clients/godot/addon
./build.sh                        # debug build, copies .so into addon/bin/
./build.sh --release              # optimised build
./build.sh --godot-project /path/to/my-game   # also installs into addons/
```

### 2. Enable the plugin

Open **Project → Project Settings → Plugins**, find **Janet World**, and click
**Enable**.

### 3. Add the node

Add a `JanetWorldClient` node anywhere in your scene tree.  Set the exported
properties in the Inspector, or leave the defaults for local development:

| Property | Default | Description |
|---|---|---|
| `endpoint` | `nats://localhost:4222` | NATS server for the janet bus |
| `session` | `default` | Janet session to join |
| `participant_id` | `godot-client` | Identity advertised on the bus |
| `auto_connect` | `true` | Connect automatically on `_ready` |

### 4. Connect signals in GDScript

```gdscript
@onready var world := $JanetWorldClient

func _ready() -> void:
    world.chunk_activated.connect(_on_chunk_activated)
    world.chunk_deactivated.connect(_on_chunk_deactivated)
    world.entity_spawned.connect(_on_entity_spawned)
    world.entity_transform.connect(_on_entity_transform)
    world.entity_removed.connect(_on_entity_removed)
    world.structure_spawned.connect(_on_structure_spawned)
    world.connection_state_changed.connect(_on_connection_changed)

func _on_connection_changed(state: String) -> void:
    print("World connection: ", state)

func _on_chunk_activated(chunk_id: String, cx: int, cy: int,
        seed: int, lod: int, chunk_size: float) -> void:
    # Generate the terrain mesh locally using the seed — no height data
    # is ever sent over the wire.
    TerrainManager.spawn_chunk(cx, cy, seed, lod, chunk_size)

func _on_chunk_deactivated(chunk_id: String) -> void:
    TerrainManager.free_chunk(chunk_id)

func _on_entity_spawned(entity_id: String, archetype: String,
        x: float, y: float, z: float, rotation_y: float) -> void:
    EntityManager.spawn(entity_id, archetype, Vector3(x, y, z), rotation_y)

func _on_entity_transform(entity_id: String,
        x: float, y: float, z: float, rotation_y: float,
        vx: float, vy: float, vz: float, frame: int, dt: float) -> void:
    EntityManager.apply_transform(entity_id, Vector3(x, y, z),
            Vector3(vx, vy, vz), rotation_y)

func _on_entity_removed(entity_id: String) -> void:
    EntityManager.despawn(entity_id)

func _on_structure_spawned(structure_id: String, type_id: String,
        x: float, y: float, z: float, rotation_y: float) -> void:
    StructureManager.spawn(structure_id, type_id, Vector3(x, y, z), rotation_y)
```

---

## API reference

### Signals

| Signal | Arguments | Emitted when |
|---|---|---|
| `connection_state_changed` | `state: String` | Connection state changes (`"connecting"`, `"active"`, `"disconnected"`, `"error"`) |
| `chunk_activated` | `chunk_id, cx, cy, seed, lod, chunk_size` | Server activates a terrain chunk |
| `chunk_deactivated` | `chunk_id` | Server deactivates a terrain chunk |
| `structure_spawned` | `structure_id, type_id, x, y, z, rotation_y` | A static structure enters the active region |
| `structure_removed` | `structure_id` | A static structure leaves the active region |
| `entity_spawned` | `entity_id, archetype, x, y, z, rotation_y` | A dynamic entity enters the active region |
| `entity_removed` | `entity_id` | A dynamic entity leaves the active region |
| `entity_transform` | `entity_id, x, y, z, rotation_y, vx, vy, vz, frame, dt` | Authoritative transform tick (~10–30 Hz) |
| `snapshot_begin` | `frame: int` | Full state snapshot started (suppress UI flicker) |
| `snapshot_end` | *(none)* | Snapshot fully applied |

### Connection methods

```gdscript
world.connect_to_world()        # connect (or reconnect)
world.disconnect_from_world()   # graceful disconnect, clears cache
```

### Intent methods

These send hints to the server.  The server holds authority — it may reject,
modify, or rate-limit any intent.

```gdscript
world.send_movement(Vector3 direction)
world.send_interaction(String target_id)
world.send_interaction_verb(String target_id, String verb)  # e.g. "open", "attack"
world.teleport(Vector3 position)         # server-authorised teleport
world.update_view_radius(float radius)   # tune server streaming density
world.request_snapshot(Vector3 pos, float radius)  # full resync (use on reconnect)
```

### Cache queries

The node maintains a local mirror of the world state that can be queried
synchronously from any GDScript — no signals required.

```gdscript
world.active_chunk_count() -> int
world.entity_count()        -> int
world.structure_count()     -> int
world.is_chunk_active(String chunk_id) -> bool
world.is_connected_to_world()          -> bool
world.last_frame()                     -> int

# Dead-reckon an entity position by `elapsed` seconds since last update.
world.extrapolate_entity(String entity_id, float elapsed) -> Vector3
```

---

## Architecture

```
Godot main thread                   Bridge thread (Tokio)
────────────────────────────────    ────────────────────────────────
JanetWorldClient._process()         BusBridge::run()
  h.poll()  ←─── crossbeam ──────── tx.send(WorldEvent)
  cache.apply(event)                  subscribe world.*
  emit_signal(...)                    parse NATS messages

  send_intent(msg) ────────────────► intent_rx.recv()
                                      client.publish(intent.*)
```

The bridge thread owns the Tokio runtime and the NATS connection.  The Godot
main thread **never touches async code** — it reads from a
`crossbeam_channel` receiver in `_process()` instead.

This guarantees:

- No Godot frame stutters from network I/O
- Safe to disconnect / reconnect at any time
- Protocol errors are logged and isolated — they never panic the main thread

### Terrain design

Raw heightmap data is **never sent over the wire**.  The server sends only
`(cx, cy, seed, lod, chunk_size)` in each `chunk_activated` event.  Clients
regenerate the mesh locally using the same deterministic noise function.
This keeps bandwidth constant regardless of terrain resolution and makes LOD
transitions trivial — just re-generate with a larger `lod` value.

---

## Directory layout

```
clients/godot/
├── Cargo.toml           # cdylib crate, not in main workspace
├── README.md
├── src/
│   ├── lib.rs           # GDExtension entry point
│   ├── node.rs          # JanetWorldClient GodotClass
│   ├── bridge.rs        # Background Tokio/NATS thread
│   ├── cache.rs         # ClientWorldCache (chunks, entities, structures)
│   └── events.rs        # WorldEvent enum (thread-safe, no-serde)
└── addon/               # Install this tree into addons/janet_world/
    ├── plugin.cfg
    ├── plugin.gd
    ├── janet_world.gdextension
    ├── JanetWorldClientHint.gd
    ├── icon.svg
    ├── dock.gd / dock.tscn
    ├── build.sh
    └── bin/             # compiled .so / .dylib / .dll (generated by build.sh)
```

---

## Requirements

| Dependency | Version |
|---|---|
| Godot | 4.2+ |
| Rust | 1.75+ |
| `janet-world` server | matching branch |
| NATS server | 2.10+ |

The Godot project needs no additional GDScript plugins.  The entire protocol
surface is exposed through the single `JanetWorldClient` node.

---

## License

MIT — see the root `Cargo.toml` for details.
