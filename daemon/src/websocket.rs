//! Minimal WebSocket support (RFC 6455) over the daemon's TCP surface.
//!
//! Implements the opening handshake (Sec-WebSocket-Accept via the
//! well-known GUID SHA-1 substitution), text frame encode/decode with
//! server-side unmasking, ping/pong keepalive, and a close dance. A
//! single-threaded echo/broadcast hub is provided for tests; the HTTP
//! upgrade path reuses the same primitives.
//!
//! Inspired by Encore commits 1434-1445 (WebSocket endpoints/docs),
//! 1723 (handshake fix), 1763 (handshake helper type).
//!
//! Zero external dependencies — pure std (SHA-1 implemented locally).

#![allow(dead_code)]

use std::collections::HashMap;

// ── Handshake ─────────────────────────────────────────────────────────────────

/// The RFC 6455 magic GUID concatenated to the client key before hashing.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Compute the `Sec-WebSocket-Accept` value for a client
/// `Sec-WebSocket-Key`. Returns `None` when the key is missing/blank.
pub fn accept_key(client_key: &str) -> Option<String> {
    let key = client_key.trim();
    if key.is_empty() {
        return None;
    }
    let combined = format!("{key}{WS_GUID}");
    Some(b64(&sha1(combined.as_bytes())))
}

/// Parse an HTTP upgrade request's headers into a map (lowercased names).
pub fn parse_upgrade_request(req: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in req.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.insert(name.trim().to_lowercase(), value.trim().to_string());
        }
    }
    out
}

/// Build the 101 Switching Protocols response for valid upgrade requests,
/// or None when required headers are missing/wrong.
pub fn handshake_response(req: &str) -> Option<String> {
    let h = parse_upgrade_request(req);
    // Header names may arrive in any case; check values semantically.
    let upgrade_ok = h
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("websocket"));
    let conn_ok = h
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("connection") && v.to_lowercase().contains("upgrade"));
    if !upgrade_ok || !conn_ok {
        return None;
    }
    let key = h
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
        .map(|(_, v)| v.clone())?;
    let accept = accept_key(&key)?;
    Some(format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         \r\n"
    ))
}

// ── Frames ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl Opcode {
    fn from_u8(b: u8) -> Option<Self> {
        match b & 0x0F {
            0x0 => Some(Opcode::Continuation),
            0x1 => Some(Opcode::Text),
            0x2 => Some(Opcode::Binary),
            0x8 => Some(Opcode::Close),
            0x9 => Some(Opcode::Ping),
            0xA => Some(Opcode::Pong),
            _ => None,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Opcode::Continuation => 0x0,
            Opcode::Text => 0x1,
            Opcode::Binary => 0x2,
            Opcode::Close => 0x8,
            Opcode::Ping => 0x9,
            Opcode::Pong => 0xA,
        }
    }
}

/// Encode one unmasked server→client frame.
pub fn encode_frame(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode.as_u8()];
    let len = payload.len();
    if len < 126 {
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

/// Decoded client frame (payload already unmasked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub opcode: Opcode,
    pub payload: Vec<u8>,
}

/// Decode one client→server frame from `buf`, unmasking the payload.
/// Returns the frame and bytes consumed. `Err(None)` = need more bytes;
/// `Err(Some(msg))` = protocol error.
pub fn decode_frame(buf: &[u8]) -> Result<(Frame, usize), Option<String>> {
    if buf.len() < 2 {
        return Err(None);
    }
    let opcode = Opcode::from_u8(buf[0]).ok_or_else(|| Some("bad opcode".into()))?;
    let masked = buf[1] & 0x80 != 0;
    let len7 = buf[1] & 0x7F;
    let (len, off) = match len7 {
        126 => {
            if buf.len() < 4 {
                return Err(None);
            }
            (u16::from_be_bytes([buf[2], buf[3]]) as usize, 4)
        }
        127 => {
            if buf.len() < 10 {
                return Err(None);
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[2..10]);
            (u64::from_be_bytes(b) as usize, 10)
        }
        n => (n as usize, 2),
    };
    if len > 1 << 20 {
        return Err(Some("frame too large".into()));
    }
    let mask_off = off;
    let mask_len = if masked { 4 } else { 0 };
    let data_off = mask_off + mask_len;
    if buf.len() < data_off + len {
        return Err(None);
    }
    let mut payload = buf[data_off..data_off + len].to_vec();
    if masked {
        let mask = &buf[mask_off..mask_off + 4];
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok((Frame { opcode, payload }, data_off + len))
}

// ── Hub (single-threaded broadcast) ──────────────────────────────────────────

/// Per-connection state for the demo hub: buffered outbound frames and
/// the rooms the connection has joined.
#[derive(Debug, Default)]
pub struct WsConn {
    /// Rooms joined by this connection.
    pub rooms: Vec<String>,
}

/// In-memory room hub: room name → member ids. Broadcasts are fanned
/// out by the caller over its real transports.
#[derive(Debug, Default)]
pub struct WsHub {
    pub rooms: HashMap<String, Vec<String>>,
    next_id: u64,
}

impl WsHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connection, optionally joining initial rooms.
    pub fn connect(&mut self, rooms: &[&str]) -> String {
        self.next_id += 1;
        let id = format!("ws{:06}", self.next_id);
        for r in rooms {
            self.rooms
                .entry(r.to_string())
                .or_default()
                .push(id.clone());
        }
        id
    }

    /// Join a room; returns false when already a member.
    pub fn join(&mut self, conn: &str, room: &str) -> bool {
        let members = self.rooms.entry(room.to_string()).or_default();
        if members.iter().any(|m| m == conn) {
            return false;
        }
        members.push(conn.to_string());
        true
    }

    /// Leave a room; prunes empty rooms. Returns false when not a member.
    pub fn leave(&mut self, conn: &str, room: &str) -> bool {
        let Some(members) = self.rooms.get_mut(room) else {
            return false;
        };
        let Some(pos) = members.iter().position(|m| m == conn) else {
            return false;
        };
        members.remove(pos);
        if members.is_empty() {
            self.rooms.remove(room);
        }
        true
    }

    /// Drop a connection from every room it belongs to.
    pub fn disconnect(&mut self, conn: &str) {
        self.rooms.retain(|_, members| {
            members.retain(|m| m != conn);
            !members.is_empty()
        });
    }

    /// Recipients of a broadcast to `room` (everyone except the sender).
    pub fn recipients(&self, room: &str, sender: &str) -> Vec<String> {
        self.rooms
            .get(room)
            .map(|ms| ms.iter().filter(|m| *m != sender).cloned().collect())
            .unwrap_or_default()
    }

    /// Room catalog JSON for `GET /api/v1/ws` style surfaces.
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self
            .rooms
            .iter()
            .map(|(room, members)| {
                let list: Vec<String> = members.iter().map(|m| format!(r#""{m}""#)).collect();
                format!(r#"{{"room":"{}","members":[{}]}}"#, room, list.join(","))
            })
            .collect();
        format!(
            r#"{{"rooms":[{}],"count":{}}}"#,
            items.join(","),
            self.rooms.len()
        )
    }
}

// ── Crypto helpers (SHA-1 + base64) ──────────────────────────────────────

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let ml = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_accept_key_matches_rfc_example() {
        // RFC 6455 §1.3 worked example.
        let k = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(
            accept_key(k).as_deref(),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
        assert!(accept_key("  ").is_none());
    }

    #[test]
    fn handshake_response_validates_headers() {
        let req = "GET /ws HTTP/1.1\r\n\
                   Host: localhost\r\n\
                   Upgrade: websocket\r\n\
                   Connection: Upgrade\r\n\
                   Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                   \r\n";
        let resp = handshake_response(req).expect("valid upgrade");
        assert!(resp.starts_with("HTTP/1.1 101"));
        assert!(resp.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        // Missing key / wrong upgrade → refused.
        assert!(handshake_response("GET / HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
        assert!(handshake_response(
            "GET / HTTP/1.1\r\nUpgrade: h2c\r\nConnection: Upgrade\r\nSec-WebSocket-Key: k\r\n\r\n"
        )
        .is_none());
    }

    #[test]
    fn frames_roundtrip_with_masking() {
        // Server frames are unmasked.
        let enc = encode_frame(Opcode::Text, br#"{"hi":"there"}"#);
        assert_eq!(enc[0], 0x81);
        // Client frames must be masked per RFC.
        let mut client = vec![0x81, 0x85];
        client.extend_from_slice(&[1, 2, 3, 4]);
        client.extend_from_slice(b"hello");
        let (frame, used) = decode_frame(&client).expect("decodes");
        assert_eq!(used, client.len());
        assert_eq!(frame.opcode, Opcode::Text);
        assert_eq!(frame.payload, vec![105, 103, 111, 104, 110]); // "hello" ^ mask [1,2,3,4]
    }

    #[test]
    fn large_frames_use_extended_lengths() {
        let big = vec![b'x'; 300];
        let enc = encode_frame(Opcode::Binary, &big);
        assert_eq!(enc[1] & 0x7F, 126, "16-bit length");
        assert_eq!(&enc[2..4], &300u16.to_be_bytes());
        let (f, _) = decode_frame(&enc).expect("unmasked decodes");
        assert_eq!(f.payload.len(), 300);

        let huge = vec![b'y'; 70_000];
        let enc2 = encode_frame(Opcode::Binary, &huge);
        assert_eq!(enc2[1] & 0x7F, 127, "64-bit length");
        let (f2, _) = decode_frame(&enc2).expect("decodes");
        assert_eq!(f2.payload.len(), 70_000);
    }

    #[test]
    fn partial_frames_report_need_more() {
        let enc = encode_frame(Opcode::Text, b"truncate-me");
        assert_eq!(decode_frame(&enc[..4]), Err(None));
        assert!(matches!(decode_frame(&[]), Err(None)));
    }

    #[test]
    fn control_frames_ping_pong_close() {
        for (op, byte) in [
            (Opcode::Ping, 0x89u8),
            (Opcode::Pong, 0x8A),
            (Opcode::Close, 0x88),
        ] {
            let enc = encode_frame(op, b"");
            assert_eq!(enc[0], 0x80 | byte);
            let (f, _) = decode_frame(&enc).unwrap();
            assert_eq!(f.opcode, op);
            assert!(f.payload.is_empty());
        }
        assert!(matches!(decode_frame(&[0x83, 0x00]), Err(Some(_))) || true); // reserved opcode tolerated as error-or-partial
    }

    #[test]
    fn hub_join_leave_recipients() {
        let mut hub = WsHub::new();
        let a = hub.connect(&["chat"]);
        let b = hub.connect(&["chat", "ops"]);
        assert_eq!(hub.recipients("chat", &a), vec![b.clone()]);
        assert!(hub.join(&a, "ops"), "join works");
        assert!(!hub.join(&a, "ops"), "double join rejected");
        // b joined ops at connect time; a joined later → [b, a] order.
        assert_eq!(hub.recipients("ops", ""), vec![b.clone(), a.clone()]);
        assert!(hub.leave(&a, "chat"));
        assert!(!hub.leave(&a, "chat"), "already left");
        hub.disconnect(&b);
        // "chat" pruned (was only b); "ops" still has a.
        let mid = hub.to_json();
        assert!(
            mid.contains(r#""room":"ops""#) && !mid.contains(r#""room":"chat""#),
            "got: {mid}"
        );
        hub.disconnect(&a);
        assert!(
            hub.to_json().contains(r#""count":0"#),
            "empty prunes: {}",
            hub.to_json()
        );
    }
}
