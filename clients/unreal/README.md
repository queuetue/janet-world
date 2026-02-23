# Janet World — Unreal Engine Client

> **Status: planned** — this client is not yet implemented.  This document
> describes the intended design and API surface to guide contributors.

An Unreal Engine 5 plugin that connects your game to a running
[janet-world](../../) server over the janet bus (NATS).

The plugin exposes a `UJanetWorldSubsystem` (Game Instance Subsystem) and a
`AJanetWorldActor` convenience actor.  Blueprint developers interact through
Delegate events; C++ developers can bind directly to the subsystem.

The implementation wraps the **janet-abi** C FFI layer (`janet-abi/`) — a
pre-compiled `.so` / `.dll` that contains the full Rust world client.  No
Rust toolchain is required in the Unreal project.

---

## Planned architecture

```
Unreal game thread
──────────────────────────────────────────
UJanetWorldSubsystem::Tick()
  └─ janet_executor_poll()  ← C FFI call into janet-abi
       returns pending WorldEvents as a C array
  └─ broadcasts UE Delegates for each event

Separate OS thread (owned by janet-abi)
  └─ Tokio runtime
       NATS TCP connection
       subscribe world.*
       event ring buffer ← polled by game thread

Game thread (intent path)
  UJanetWorldSubsystem::SendMovement(FVector)
  └─ janet_executor_publish()  ← C FFI
       serialises IntentMove JSON
       queues for NATS publish
```

---

## Quick start (once implemented)

### 1. Obtain the pre-built library

```bash
# From the janet-world repo root:
cargo build -p janet-abi --release
# Outputs: target/release/libjanet_abi.so  (Linux)
#          target/release/janet_abi.dll    (Windows)
#          target/release/libjanet_abi.dylib (macOS)
```

Copy the library into `Plugins/JanetWorld/Binaries/<Platform>/`.

### 2. Add the plugin to your project

Copy the `clients/unreal/JanetWorld/` folder into `YourGame/Plugins/`.
Right-click `YourGame.uproject` → *Generate Visual Studio project files*.

### 3. Configure (Blueprint or C++)

**Blueprint:**

1. Add a `Janet World Actor` to your level.
2. Set `Endpoint`, `Session`, and `Participant Id` in the Details panel.
3. Bind to the delegates in the actor's Event Graph.

**C++:**

```cpp
#include "JanetWorldSubsystem.h"

void AMyGameMode::BeginPlay()
{
    Super::BeginPlay();

    auto* World = GetGameInstance()->GetSubsystem<UJanetWorldSubsystem>();

    World->OnChunkActivated.AddDynamic(this, &AMyGameMode::HandleChunkActivated);
    World->OnEntityTransform.AddDynamic(this, &AMyGameMode::HandleEntityTransform);

    World->Connect(TEXT("nats://localhost:4222"), TEXT("default"), TEXT("ue5-client"));
}

void AMyGameMode::HandleChunkActivated(
    const FString& ChunkId, int32 CX, int32 CY,
    int64 Seed, int32 Lod, float ChunkSize)
{
    // Generate terrain procedurally from seed — no heightmap over the wire.
    TerrainManager->SpawnChunk(CX, CY, Seed, Lod, ChunkSize);
}
```

---

## Planned API

### `UJanetWorldSubsystem` (C++ / Blueprint)

#### Configuration

| Property | Default | Description |
|---|---|---|
| `Endpoint` | `nats://localhost:4222` | NATS server address |
| `Session` | `default` | Janet session name |
| `ParticipantId` | `ue5-client` | Identity on the bus |
| `AutoConnect` | `true` | Connect on subsystem Init |

#### Delegates

| Delegate | Signature | Fired when |
|---|---|---|
| `OnConnectionState` | `FString State` | State changes: `Connecting`, `Active`, `Disconnected`, `Error` |
| `OnChunkActivated` | `FString ChunkId, int32 CX, int32 CY, int64 Seed, int32 Lod, float ChunkSize` | Terrain chunk activated |
| `OnChunkDeactivated` | `FString ChunkId` | Terrain chunk freed |
| `OnStructureSpawned` | `FString StructureId, FString TypeId, FVector Location, float RotY` | Structure appeared |
| `OnStructureRemoved` | `FString StructureId` | Structure removed |
| `OnEntitySpawned` | `FString EntityId, FString Archetype, FVector Location, float RotY` | Entity appeared |
| `OnEntityRemoved` | `FString EntityId` | Entity disappeared |
| `OnEntityTransform` | `FString EntityId, FVector Location, float RotY, FVector Velocity, int64 Frame, float Dt` | Authoritative transform |
| `OnSnapshotBegin` | `int64 Frame` | Full snapshot started |
| `OnSnapshotEnd` | *(none)* | Full snapshot complete |

#### Methods

```cpp
// Connection
void Connect(FString Endpoint, FString Session, FString ParticipantId);
void Disconnect();
bool IsConnected() const;

// Intents
void SendMovement(FVector Direction);
void SendInteraction(FString TargetId, FString Verb = TEXT(""));
void Teleport(FVector Position);
void UpdateViewRadius(float Radius);
void RequestSnapshot(FVector Position, float Radius);

// Cache queries
int32 GetActiveChunkCount() const;
int32 GetEntityCount() const;
bool IsChunkActive(FString ChunkId) const;
int64 GetLastFrame() const;
FVector ExtrapolateEntity(FString EntityId, float ElapsedSeconds) const;
```

---

## Directory layout (planned)

```
clients/unreal/
├── README.md
└── JanetWorld/                 ← drop this into YourGame/Plugins/
    ├── JanetWorld.uplugin
    ├── Binaries/               ← pre-built janet-abi libraries
    │   ├── Win64/
    │   ├── Linux/
    │   └── Mac/
    ├── Source/
    │   └── JanetWorld/
    │       ├── JanetWorld.Build.cs
    │       ├── Public/
    │       │   ├── JanetWorldSubsystem.h
    │       │   ├── JanetWorldActor.h
    │       │   └── JanetWorldTypes.h
    │       └── Private/
    │           ├── JanetWorldSubsystem.cpp
    │           └── JanetWorldActor.cpp
    └── janet_abi/              ← C headers from janet-abi/include/
        └── janet_abi.h
```

---

## Terrain design

Same as all other clients: only `(CX, CY, Seed, Lod, ChunkSize)` is sent
over the wire.  The UE project uses a procedural mesh component (or
Landscape tile) generated locally from the seed — no heightmap transfer.

---

## Contributing

1. The wire protocol is fully defined in [`../../src/protocol.rs`](../../src/protocol.rs).
2. The C FFI surface is in [`../../../janet-abi/include/`](../../../janet-abi/include/).
3. The Godot client (`../godot/src/`) is the reference for event handling and
   cache management — the C++ translation is mechanical.
4. UE minimum version: **5.2** (Game Instance Subsystems, Enhanced Input).

---

## Requirements (planned)

| Dependency | Version |
|---|---|
| Unreal Engine | 5.2+ |
| janet-abi (pre-built) | matching branch |
| NATS server | 2.10+ |

---

## License

MIT — see the root `Cargo.toml` for details.
