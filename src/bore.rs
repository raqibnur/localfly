//! bore v0.6 wire protocol, ported from the original macOS app Swift core (BoreProtocol.swift).
//!
//! Every control message is JSON encoded like serde_json encodes a Rust enum,
//! terminated by one NUL byte. Unit variants are bare strings ("Heartbeat"),
//! newtype variants are single-key objects ({"Hello":3000}).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const CONTROL_PORT: u16 = 7835;
pub const MAX_FRAME_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    Challenge(Uuid),
    Hello(u16),
    Heartbeat,
    Connection(Uuid),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    FrameTooLong,
    Malformed(String),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameTooLong => write!(f, "Relay sent an oversized control frame"),
            Self::Malformed(d) => write!(f, "Relay sent an unreadable message ({d})"),
        }
    }
}

impl std::error::Error for ProtocolError {}

pub fn hello(port: u16) -> Vec<u8> {
    frame(&format!(r#"{{"Hello":{port}}}"#))
}

pub fn accept(id: &Uuid) -> Vec<u8> {
    frame(&format!(r#"{{"Accept":"{}"}}"#, id.hyphenated().to_string().to_lowercase()))
}

pub fn authenticate(tag: &str) -> Vec<u8> {
    frame(&format!(r#"{{"Authenticate":"{tag}"}}"#))
}

/// HMAC-SHA256 over the 16 raw challenge bytes, keyed with SHA256(secret), hex encoded.
/// Must match Swift `BoreProtocol.answer` and the Python reference in BoreProtocolTests.
pub fn answer(challenge: &Uuid, secret: &str) -> String {
    let key_bytes = Sha256::digest(secret.as_bytes());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key_bytes).expect("HMAC takes any key size");
    mac.update(challenge.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn decode(frame: &[u8]) -> Result<ServerMessage, ProtocolError> {
    let v: serde_json::Value =
        serde_json::from_slice(frame).map_err(|_| ProtocolError::Malformed("invalid JSON".into()))?;
    if let Some(unit) = v.as_str() {
        if unit == "Heartbeat" {
            return Ok(ServerMessage::Heartbeat);
        }
        return Err(ProtocolError::Malformed(format!("unknown message {unit}")));
    }
    let obj = v.as_object().ok_or_else(|| ProtocolError::Malformed("unexpected shape".into()))?;
    if obj.len() != 1 {
        return Err(ProtocolError::Malformed("unexpected shape".into()));
    }
    let (key, payload) = obj.iter().next().unwrap();
    match key.as_str() {
        "Hello" => {
            let port = payload.as_u64().and_then(|p| u16::try_from(p).ok()).ok_or_else(|| {
                // serde_json parses large numbers as u64; reject > 65535 like Swift's UInt16(exactly:)
                ProtocolError::Malformed("bad port".into())
            })?;
            Ok(ServerMessage::Hello(port))
        }
        "Challenge" | "Connection" => {
            let text = payload
                .as_str()
                .ok_or_else(|| ProtocolError::Malformed("bad id".into()))?;
            let id =
                Uuid::parse_str(text).map_err(|_| ProtocolError::Malformed("bad id".into()))?;
            if key == "Challenge" {
                Ok(ServerMessage::Challenge(id))
            } else {
                Ok(ServerMessage::Connection(id))
            }
        }
        "Error" => Ok(ServerMessage::Error(payload.as_str().unwrap_or("unknown error").to_string())),
        other => Err(ProtocolError::Malformed(format!("unknown message {other}"))),
    }
}

fn frame(json: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(json.len() + 1);
    v.extend_from_slice(json.as_bytes());
    v.push(0);
    v
}

/// Splits a byte stream into NUL-delimited control frames.
#[derive(Debug, Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends bytes, returns every complete frame now available.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&b| b == 0) {
            let frame = self.buffer[..end].to_vec();
            if frame.len() > MAX_FRAME_LENGTH {
                return Err(ProtocolError::FrameTooLong);
            }
            frames.push(frame);
            self.buffer.drain(..=end);
        }
        if self.buffer.len() > MAX_FRAME_LENGTH {
            return Err(ProtocolError::FrameTooLong);
        }
        Ok(frames)
    }

    /// Bytes received after the last complete frame.
    pub fn drain(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }
}

/// Parse `host` or `host:port` (control port, default 7835).
/// Mirrors Swift `TunnelOptions.relayEndpoint`.
pub fn parse_relay(relay: &str) -> (String, u16) {
    let trimmed = relay.trim();
    if let Some(colon) = trimmed.rfind(':') {
        if let Ok(port) = trimmed[colon + 1..].parse::<u16>() {
            // Swift uses lastIndex(of: ":") unconditionally, so "[::1]:1234"
            // style brackets are not specially handled there either.
            return (trimmed[..colon].to_string(), port);
        }
    }
    if trimmed.is_empty() {
        ("bore.pub".to_string(), CONTROL_PORT)
    } else {
        (trimmed.to_string(), CONTROL_PORT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_serde() {
        assert_eq!(hello(0), b"{\"Hello\":0}\0".to_vec());
        let id = Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
        assert_eq!(accept(&id), b"{\"Accept\":\"67e55044-10b1-426f-9247-bb680e5fe0c8\"}\0".to_vec());
    }

    #[test]
    fn decodes_server_messages() {
        let id = "67e55044-10b1-426f-9247-bb680e5fe0c8";
        let uuid = Uuid::parse_str(id).unwrap();
        assert_eq!(decode(br#""Heartbeat""#).unwrap(), ServerMessage::Heartbeat);
        assert_eq!(decode(br#"{"Hello":41234}"#).unwrap(), ServerMessage::Hello(41234));
        assert_eq!(
            decode(format!(r#"{{"Connection":"{id}"}}"#).as_bytes()).unwrap(),
            ServerMessage::Connection(uuid)
        );
        assert_eq!(
            decode(format!(r#"{{"Challenge":"{id}"}}"#).as_bytes()).unwrap(),
            ServerMessage::Challenge(uuid)
        );
        assert_eq!(
            decode(br#"{"Error":"port already in use"}"#).unwrap(),
            ServerMessage::Error("port already in use".into())
        );
        assert!(decode(br#"{"Hello":70000}"#).is_err());
    }

    #[test]
    fn answers_challenge_like_python_reference() {
        // Reference from Python: hmac.new(sha256(b"s3cret").digest(), uuid.bytes, sha256).hexdigest()
        let challenge = Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
        assert_eq!(
            answer(&challenge, "s3cret"),
            "9d012242868ec7ae18e13fcc858d7d8a7003339a47723aab17aee26a30ad4723"
        );
    }

    #[test]
    fn splits_frames_across_chunks() {
        let mut reader = FrameReader::new();
        assert!(reader.push(b"\"Heart").unwrap().is_empty());
        let mut chunk = b"beat\"".to_vec();
        chunk.push(0);
        chunk.extend_from_slice(br#"{"Hello":1}"#);
        chunk.push(0);
        chunk.extend_from_slice(b"raw");
        let frames = reader.push(&chunk).unwrap();
        assert_eq!(frames, vec![br#""Heartbeat""#.to_vec(), br#"{"Hello":1}"#.to_vec()]);
        assert_eq!(reader.drain(), b"raw");
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut reader = FrameReader::new();
        assert_eq!(
            reader.push(&vec![65u8; MAX_FRAME_LENGTH + 1]).unwrap_err(),
            ProtocolError::FrameTooLong
        );
    }

    #[test]
    fn parses_relay_endpoint() {
        assert_eq!(parse_relay("bore.pub"), ("bore.pub".into(), 7835));
        assert_eq!(parse_relay(" relay.example.com:9000 "), ("relay.example.com".into(), 9000));
        assert_eq!(parse_relay(""), ("bore.pub".into(), 7835));
    }
}
