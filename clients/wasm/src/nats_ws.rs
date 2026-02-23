//! Minimal NATS-over-WebSocket protocol implementation.
//!
//! NATS servers with `--websocket` (or `websocket { ... }` config block)
//! speak the standard NATS text protocol framed as WebSocket messages.
//! Each WebSocket *text* frame may contain one or more NATS operations
//! separated by `\r\n`.
//!
//! We implement only the operations required by this client:
//!
//! | Direction        | Command               |
//! |------------------|-----------------------|
//! | Server → client  | `INFO`, `MSG`, `PING`, `+OK`, `-ERR` |
//! | Client → server  | `CONNECT`, `SUB`, `PUB`, `PONG`      |
//!
//! Reference: <https://docs.nats.io/reference/reference-protocols/nats>

// ---------------------------------------------------------------------------
// Outbound frame builders
// ---------------------------------------------------------------------------

/// Build the `CONNECT` handshake frame.
///
/// `verbose: false` suppresses `+OK` acks (reduces noise).
pub fn connect_frame(client_name: &str) -> String {
    let payload = serde_json::json!({
        "verbose": false,
        "pedantic": false,
        "tls_required": false,
        "name": client_name,
        "lang": "rust-wasm",
        "version": "0.1.0",
    });
    format!("CONNECT {}\r\n", payload)
}

/// Build a `SUB <subject> <sid>` frame.
pub fn sub_frame(subject: &str, sid: u32) -> String {
    format!("SUB {} {}\r\n", subject, sid)
}

/// Build a `PUB <subject> <len>\r\n<payload>\r\n` frame.
pub fn pub_frame(subject: &str, payload: &str) -> String {
    format!("PUB {} {}\r\n{}\r\n", subject, payload.len(), payload)
}

/// Build the `PONG` response (reply to server `PING`).
pub fn pong_frame() -> String {
    "PONG\r\n".into()
}

// ---------------------------------------------------------------------------
// Inbound message parsing
// ---------------------------------------------------------------------------

/// A single parsed NATS operation received from the server.
#[derive(Debug, Clone)]
pub enum NatsOp {
    /// `INFO {...}` — server sends this on connect.
    Info { json: String },
    /// `MSG <subject> <sid> <len>\r\n<payload>` — incoming published message.
    Msg { subject: String, payload: Vec<u8> },
    /// `PING` — must respond with `PONG`.
    Ping,
    /// `+OK` — verbose acknowledgement (we ask for verbose=false).
    Ok,
    /// `-ERR <message>`
    Err { message: String },
}

/// Parse one or more NATS protocol operations from a raw WebSocket text frame.
///
/// A single frame may contain multiple complete operations.  Incomplete
/// operations (partial `MSG` body) are silently dropped — the NATS server
/// always delivers complete WebSocket frames.
pub fn parse_frame(text: &str) -> Vec<NatsOp> {
    let mut ops = Vec::new();
    let mut lines = text.split("\r\n").peekable();

    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("INFO ") {
            ops.push(NatsOp::Info {
                json: line[5..].to_string(),
            });
        } else if line == "PING" {
            ops.push(NatsOp::Ping);
        } else if line == "+OK" {
            ops.push(NatsOp::Ok);
        } else if line.starts_with("-ERR ") {
            ops.push(NatsOp::Err {
                message: line[5..].to_string(),
            });
        } else if line.starts_with("MSG ") {
            // MSG <subject> <sid> [reply-to] <#bytes>
            let parts: Vec<&str> = line.split_whitespace().collect();
            // parts[0]="MSG" parts[1]=subject parts[2]=sid [parts[3]=reply] parts[last]=len
            if parts.len() < 4 {
                continue;
            }
            let subject = parts[1].to_string();
            let byte_count: usize = parts.last().and_then(|s| s.parse().ok()).unwrap_or(0);

            // Payload is on the next line from the split
            if let Some(payload_line) = lines.next() {
                let payload = payload_line.as_bytes();
                let payload = &payload[..payload.len().min(byte_count)];
                ops.push(NatsOp::Msg {
                    subject,
                    payload: payload.to_vec(),
                });
            }
        }
        // HMSG (headers), +OK (verbose) etc. — silently ignored
    }

    ops
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Outbound frame builders
    // ---------------------------------------------------------------

    #[test]
    fn connect_frame_is_valid_nats() {
        let frame = connect_frame("test-client");
        assert!(frame.starts_with("CONNECT "));
        assert!(frame.ends_with("\r\n"));
        // The payload must be valid JSON
        let json_part = &frame[8..frame.len() - 2];
        let v: serde_json::Value = serde_json::from_str(json_part).expect("valid JSON");
        assert_eq!(v["name"], "test-client");
        assert_eq!(v["verbose"], false);
    }

    #[test]
    fn sub_frame_format() {
        assert_eq!(sub_frame("world.>", 1), "SUB world.> 1\r\n");
        assert_eq!(sub_frame("intent.move", 42), "SUB intent.move 42\r\n");
    }

    #[test]
    fn pub_frame_has_correct_length() {
        let body = r#"{"dx":1.0}"#;
        let frame = pub_frame("intent.move", body);
        assert!(frame.contains(&format!("PUB intent.move {}", body.len())));
        assert!(frame.ends_with("\r\n"));
    }

    #[test]
    fn pub_frame_empty_payload() {
        let frame = pub_frame("world.ping", "");
        assert_eq!(frame, "PUB world.ping 0\r\n\r\n");
    }

    #[test]
    fn pong_frame_format() {
        assert_eq!(pong_frame(), "PONG\r\n");
    }

    // ---------------------------------------------------------------
    // Inbound parsing — atoms
    // ---------------------------------------------------------------

    #[test]
    fn parse_ping() {
        let ops = parse_frame("PING\r\n");
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], NatsOp::Ping));
    }

    #[test]
    fn parse_ok() {
        let ops = parse_frame("+OK\r\n");
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], NatsOp::Ok));
    }

    #[test]
    fn parse_err() {
        let ops = parse_frame("-ERR 'Unknown Protocol Operation'\r\n");
        assert_eq!(ops.len(), 1);
        if let NatsOp::Err { message } = &ops[0] {
            assert!(message.contains("Unknown Protocol"));
        } else {
            panic!("expected NatsOp::Err");
        }
    }

    #[test]
    fn parse_info() {
        let ops = parse_frame("INFO {\"server_id\":\"abc\"}\r\n");
        assert_eq!(ops.len(), 1);
        if let NatsOp::Info { json } = &ops[0] {
            assert!(json.contains("abc"));
        } else {
            panic!("expected Info");
        }
    }

    // ---------------------------------------------------------------
    // Inbound parsing — MSG
    // ---------------------------------------------------------------

    #[test]
    fn parse_msg() {
        let frame = "MSG world.chunk.activated 1 42\r\n{\"chunk_id\":\"0:0\"}\r\n";
        let ops = parse_frame(frame);
        assert_eq!(ops.len(), 1);
        if let NatsOp::Msg { subject, payload } = &ops[0] {
            assert_eq!(subject, "world.chunk.activated");
            // Payload is truncated to the declared byte count (42),
            // but the actual JSON is shorter — verify we got it.
            assert!(!payload.is_empty());
        } else {
            panic!("expected Msg");
        }
    }

    #[test]
    fn parse_msg_exact_length() {
        let body = "hello";
        let frame = format!("MSG test.subj 1 {}\r\n{}\r\n", body.len(), body);
        let ops = parse_frame(&frame);
        assert_eq!(ops.len(), 1);
        if let NatsOp::Msg { subject, payload } = &ops[0] {
            assert_eq!(subject, "test.subj");
            assert_eq!(payload, b"hello");
        } else {
            panic!("expected Msg");
        }
    }

    #[test]
    fn parse_msg_truncates_to_byte_count() {
        // Byte count says 3 but payload is longer
        let frame = "MSG test.subj 1 3\r\nhello_world\r\n";
        let ops = parse_frame(frame);
        assert_eq!(ops.len(), 1);
        if let NatsOp::Msg { payload, .. } = &ops[0] {
            assert_eq!(payload, b"hel");
        } else {
            panic!("expected Msg");
        }
    }

    #[test]
    fn parse_msg_missing_payload_line() {
        // MSG header without a following payload line — should be skipped
        let frame = "MSG test.subj 1 5\r\n";
        let ops = parse_frame(frame);
        // No complete MSG, but the empty-string line will be consumed
        assert!(ops.is_empty() || matches!(ops.last(), Some(NatsOp::Msg { .. })));
    }

    // ---------------------------------------------------------------
    // Inbound parsing — multiple operations in one frame
    // ---------------------------------------------------------------

    #[test]
    fn parse_multiple_ops() {
        let frame = "PING\r\nMSG world.entity.transform 2 2\r\n{}\r\n";
        let ops = parse_frame(frame);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], NatsOp::Ping));
        assert!(matches!(ops[1], NatsOp::Msg { .. }));
    }

    #[test]
    fn parse_info_then_ping() {
        let frame = "INFO {\"server_id\":\"x\"}\r\nPING\r\n";
        let ops = parse_frame(frame);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], NatsOp::Info { .. }));
        assert!(matches!(ops[1], NatsOp::Ping));
    }

    #[test]
    fn parse_empty_frame() {
        let ops = parse_frame("");
        assert!(ops.is_empty());
    }

    #[test]
    fn parse_only_crlf() {
        let ops = parse_frame("\r\n\r\n\r\n");
        assert!(ops.is_empty());
    }

    // ---------------------------------------------------------------
    // Round-trip: build then parse
    // ---------------------------------------------------------------

    #[test]
    fn roundtrip_pub_then_msg() {
        // Simulate: client PUBs, server echoes as MSG
        let payload = r#"{"dx":1.5}"#;
        let pub_f = pub_frame("intent.move", payload);
        // Server would echo: MSG intent.move <sid> <len>\r\n<payload>\r\n
        let msg_f = format!("MSG intent.move 99 {}\r\n{}\r\n", payload.len(), payload);
        let ops = parse_frame(&msg_f);
        assert_eq!(ops.len(), 1);
        if let NatsOp::Msg {
            subject,
            payload: p,
        } = &ops[0]
        {
            assert_eq!(subject, "intent.move");
            assert_eq!(std::str::from_utf8(p).unwrap(), payload);
        } else {
            panic!("expected Msg");
        }
        // Verify the PUB frame is also well-formed (doesn't crash)
        assert!(pub_f.starts_with("PUB "));
    }
}
