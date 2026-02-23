//! Bus integration – WorldBusAgent connects as an external physics participant.
//!
//! ## Role on the bus
//!
//! The world service joins a session with role `world` and capability
//! `external_physics: true`.  The coordinator delegates spatial authority to
//! this participant; the world service in turn drives [`WorldService`] based
//! on bus events.
//!
//! ## Event contract (inbound)
//!
//! | Command                   | Payload keys              | Effect                        |
//! |---------------------------|---------------------------|-------------------------------|
//! | `world.participant.join`  | id, x, y, z              | `register_participant`        |
//! | `world.participant.leave` | id                        | `unregister_participant`      |
//! | `world.command.teleport`  | id, x, y, z              | forces position update        |
//! | `world.command.stats`     | *(empty)*                 | reply with `WorldStats`       |
//!
//! ## Event contract (outbound)
//!
//! | Subject                      | Payload type                          |
//! |------------------------------|---------------------------------------|
//! | `world.chunk.activated`      | `WorldEvent<ChunkActivated>`          |
//! | `world.chunk.deactivated`    | `WorldEvent<ChunkDeactivated>`        |
//! | `world.entity.transform`     | `WorldEvent<EntityTransform>`         |
//! | `world.snapshot` (cmd reply) | `WorldSnapshot` (via cmd response)    |

use crate::protocol::subjects::mgmt;
use crate::protocol::{subjects, WorldEvent};
use crate::service::WorldService;
use crate::types::{Vec3, WorldStats};
use anyhow::{Context, Result};
use bytes::Bytes;
use log::info;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Wire messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantJoinMsg {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantLeaveMsg {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleportMsg {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

// ---------------------------------------------------------------------------
// Config for WorldBusAgent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WorldBusConfig {
    /// Janet session to join.
    pub session: String,
    /// Participant ID advertised on the bus.
    pub participant_id: String,
    /// NATS (or other backend) endpoint.
    pub endpoint: String,
    /// Tick rate in Hz.
    pub tick_rate_hz: f32,
}

impl Default for WorldBusConfig {
    fn default() -> Self {
        Self {
            session: "default".into(),
            participant_id: "world-service".into(),
            endpoint: "nats://localhost:4222".into(),
            tick_rate_hz: 30.0,
        }
    }
}

// ---------------------------------------------------------------------------
// WorldBusAgent
// ---------------------------------------------------------------------------

/// Wraps a [`WorldService`] and drives it from janet bus events.
///
/// Call [`WorldBusAgent::run`] inside a Tokio task to start the agent.
pub struct WorldBusAgent {
    config: WorldBusConfig,
    service: Arc<Mutex<WorldService>>,
}

impl WorldBusAgent {
    pub fn new(config: WorldBusConfig, service: Arc<Mutex<WorldService>>) -> Self {
        Self { config, service }
    }

    /// Start the agent.  Connects to the bus, registers as an external
    /// physics participant, and runs the tick loop until the task is cancelled.
    ///
    /// Uses `janet_client::JanetExecutor` — same pattern as the coordinator.
    pub async fn run(self) -> Result<()> {
        use janet_client::messages::CommandResponse;
        use janet_client::{ClientBuilder, JanetExecutor};

        info!(
            "WorldBusAgent connecting as '{}' in session '{}'",
            self.config.participant_id, self.config.session
        );

        let client: JanetExecutor = ClientBuilder::new()
            .session(&self.config.session)
            .participant(&self.config.participant_id, vec!["world".to_string()])
            .capability("external_physics", true)
            .capability("world_engine", "janet-world")
            .connect()
            .await
            .context("Failed to connect world service to janet bus")?;

        info!(
            "WorldBusAgent active – ticking at {:.0}Hz",
            self.config.tick_rate_hz
        );

        // -----------------------------------------------------------------------
        // Register command handlers (synchronous registration)
        // -----------------------------------------------------------------------

        // world.command.stats
        {
            let svc = self.service.clone();
            client.on_command(mgmt::STATS, move |cmd| {
                let stats: WorldStats = svc.lock().stats();
                let result = serde_json::to_value(&stats).ok();
                async move { Ok(CommandResponse::success(cmd.command_id, result)) }
            });
        }

        // world.cmd.snapshot – full state dump for a reconnecting client
        {
            let svc = self.service.clone();
            let session = self.config.session.clone();
            client.on_command(subjects::CMD_SNAPSHOT, move |cmd| {
                let svc = svc.clone();
                let session = session.clone();
                async move {
                    let snapshot = svc.lock().build_snapshot(&session);
                    let result = serde_json::to_value(&snapshot).ok();
                    Ok(CommandResponse::success(cmd.command_id, result))
                }
            });
        }

        // world.participant.join
        {
            let svc = self.service.clone();
            client.on_command(mgmt::PARTICIPANT_JOIN, move |cmd| {
                let payload_val =
                    serde_json::Value::Object(cmd.payload.clone().into_iter().collect());
                let svc = svc.clone();
                async move {
                    match serde_json::from_value::<ParticipantJoinMsg>(payload_val) {
                        Ok(m) => {
                            svc.lock()
                                .register_participant(m.id, Vec3::new(m.x, m.y, m.z));
                            Ok(CommandResponse::success(cmd.command_id, None))
                        }
                        Err(e) => Ok(CommandResponse::failed(
                            cmd.command_id,
                            format!("Invalid payload: {}", e),
                        )),
                    }
                }
            });
        }

        // world.participant.leave
        {
            let svc = self.service.clone();
            client.on_command(mgmt::PARTICIPANT_LEAVE, move |cmd| {
                let payload_val =
                    serde_json::Value::Object(cmd.payload.clone().into_iter().collect());
                let svc = svc.clone();
                async move {
                    match serde_json::from_value::<ParticipantLeaveMsg>(payload_val) {
                        Ok(m) => {
                            svc.lock().unregister_participant(&m.id);
                            Ok(CommandResponse::success(cmd.command_id, None))
                        }
                        Err(e) => Ok(CommandResponse::failed(
                            cmd.command_id,
                            format!("Invalid payload: {}", e),
                        )),
                    }
                }
            });
        }

        // world.command.teleport
        {
            let svc = self.service.clone();
            client.on_command(mgmt::TELEPORT, move |cmd| {
                let payload_val =
                    serde_json::Value::Object(cmd.payload.clone().into_iter().collect());
                let svc = svc.clone();
                async move {
                    match serde_json::from_value::<TeleportMsg>(payload_val) {
                        Ok(m) => {
                            svc.lock()
                                .register_participant(m.id, Vec3::new(m.x, m.y, m.z));
                            Ok(CommandResponse::success(cmd.command_id, None))
                        }
                        Err(e) => Ok(CommandResponse::failed(
                            cmd.command_id,
                            format!("Invalid payload: {}", e),
                        )),
                    }
                }
            });
        }

        // -----------------------------------------------------------------------
        // Spawn world tick loop
        // -----------------------------------------------------------------------

        let service_tick = self.service.clone();
        let tick_hz = self.config.tick_rate_hz;
        let tick_client = client.clone();
        let tick_session = self.config.session.clone();

        let tick_handle = tokio::spawn(async move {
            let interval = std::time::Duration::from_secs_f32(1.0 / tick_hz);
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;

                // Hold the lock only long enough to tick, then release before publishing.
                let tick_result = {
                    let mut svc = service_tick.lock();
                    svc.tick()
                };

                match tick_result {
                    Ok(events) => {
                        let frame = events.tick;
                        let session = tick_session.as_str();

                        // --- chunk.activated ---
                        for chunk in &events.activated {
                            publish_event(
                                &tick_client,
                                subjects::CHUNK_ACTIVATED,
                                WorldEvent::new(session, frame, chunk),
                            )
                            .await;
                        }

                        // --- chunk.deactivated ---
                        for chunk in &events.deactivated {
                            publish_event(
                                &tick_client,
                                subjects::CHUNK_DEACTIVATED,
                                WorldEvent::new(session, frame, chunk),
                            )
                            .await;
                        }

                        // --- entity.transform (every participant, every tick) ---
                        for transform in &events.entity_transforms {
                            publish_event(
                                &tick_client,
                                subjects::ENTITY_TRANSFORM,
                                WorldEvent::new(session, frame, transform),
                            )
                            .await;
                        }
                    }
                    Err(e) => log::warn!("World tick error: {}", e),
                }
            }
        });

        // -----------------------------------------------------------------------
        // Wait for shutdown signal
        // -----------------------------------------------------------------------

        tokio::select! {
            _ = tick_handle => {
                log::error!("World tick loop exited unexpectedly");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("WorldBusAgent shutting down (SIGINT)");
            }
        }

        // Drop client to gracefully close the connection.
        drop(client);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Publish helper
// ---------------------------------------------------------------------------

/// Serialise `event` and publish it on `subject`.
///
/// Errors are logged and swallowed — a single failed publish should not crash
/// the tick loop.
async fn publish_event<T: serde::Serialize>(
    client: &janet_client::JanetExecutor,
    subject: &str,
    event: WorldEvent<T>,
) {
    match serde_json::to_vec(&event) {
        Ok(payload) => {
            if let Err(e) = client.publish(subject, Bytes::from(payload)).await {
                log::warn!("Failed to publish to {}: {}", subject, e);
            }
        }
        Err(e) => log::warn!("Failed to serialise event for {}: {}", subject, e),
    }
}
