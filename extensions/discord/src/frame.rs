//! Discord's IPC frames: an op and the length of what follows, each a
//! little-endian u32, then that much JSON.

use serde_json::Value;

/// The first frame: `{"v": 1, "client_id": ...}`.
pub const HANDSHAKE: u32 = 0;
/// A command, or Discord's answer to one, or what it dispatches.
pub const FRAME: u32 = 1;
/// Either end is done, and says why.
pub const CLOSE: u32 = 2;
pub const PING: u32 = 3;
pub const PONG: u32 = 4;

/// The longest frame taken: Discord's are a few KB, so one longer is a
/// stream gone wrong.
pub const MAX: usize = 1 << 20;

/// A frame, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub op: u32,
    pub payload: Value,
}

/// `payload` as a frame of `op`.
pub fn encode(op: u32, payload: &Value) -> Vec<u8> {
    let json = payload.to_string();
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

/// What came on the socket, read a frame at a time as each has all come.
#[derive(Debug, Default)]
pub struct Reader {
    bytes: Vec<u8>,
}

impl Reader {
    pub fn push(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
    }

    /// The next frame, once it's all there; an error when what came isn't
    /// a frame, after which nothing more can be read.
    pub fn take(&mut self) -> Option<Result<Frame, String>> {
        if self.bytes.len() < 8 {
            return None;
        }
        let word = |at: usize| u32::from_le_bytes([self.bytes[at], self.bytes[at + 1], self.bytes[at + 2], self.bytes[at + 3]]);
        let (op, len) = (word(0), word(4) as usize);
        if len > MAX {
            return Some(Err(format!("a frame of {len} bytes")));
        }
        if self.bytes.len() < 8 + len {
            return None;
        }
        let body: Vec<u8> = self.bytes.drain(..8 + len).skip(8).collect();
        Some(serde_json::from_slice(&body).map(|payload| Frame { op, payload }).map_err(|e| format!("op {op}: {e}")))
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_frame_is_op_length_and_json_little_endian() {
        let bytes = encode(HANDSHAKE, &json!({ "v": 1 }));
        assert_eq!(&bytes[..8], &[0, 0, 0, 0, 7, 0, 0, 0]);
        assert_eq!(&bytes[8..], br#"{"v":1}"#);
        assert_eq!(&encode(PONG, &json!(null))[..4], &[4, 0, 0, 0]);
    }

    #[test]
    fn frames_read_back_however_they_come() {
        let mut stream = encode(FRAME, &json!({ "evt": "READY" }));
        stream.extend(encode(PING, &json!({ "n": 1 })));
        let mut reader = Reader::default();
        // A byte at a time: nothing until a frame's all there.
        let mut read = Vec::new();
        for b in &stream {
            reader.push(&[*b]);
            while let Some(frame) = reader.take() {
                read.push(frame.unwrap());
            }
        }
        assert_eq!(read, [Frame { op: FRAME, payload: json!({ "evt": "READY" }) }, Frame { op: PING, payload: json!({ "n": 1 }) }]);
        assert!(reader.take().is_none());
    }

    #[test]
    fn what_isnt_a_frame_is_an_error() {
        let mut reader = Reader::default();
        reader.push(&[1, 0, 0, 0, 0xff, 0xff, 0xff, 0x7f]);
        assert!(reader.take().unwrap().unwrap_err().contains("bytes"));
        let mut reader = Reader::default();
        let mut bad = vec![1, 0, 0, 0, 3, 0, 0, 0];
        bad.extend(b"{x}");
        reader.push(&bad);
        assert!(reader.take().unwrap().is_err());
    }
}
