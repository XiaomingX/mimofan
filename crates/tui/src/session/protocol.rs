//! Framed protocol over the Unix socket.
//!
//! All messages are length-prefixed so PTY byte streams (which may contain
//! arbitrary data) cannot be confused with control messages.
//!
//! ```text
//! [1 byte opcode][4 bytes big-endian length][payload]
//! ```
//!
//! Opcodes:
//! - `0x01` DATA     payload bytes flow between client and PTY.
//! - `0x02` DETACH   client asks to detach; daemon keeps the PTY alive.
//! - `0x03` RESIZE   payload = 2 × u16 (rows, cols) for PTY resize.
//! - `0x04` SHUTDOWN daemon terminates the session (used by `session kill`).

use std::io::{self, Read, Write};

pub const OP_DATA: u8 = 0x01;
pub const OP_DETACH: u8 = 0x02;
pub const OP_RESIZE: u8 = 0x03;
pub const OP_SHUTDOWN: u8 = 0x04;

const HEADER_LEN: usize = 5;

/// Encode a frame into `buf` (opcode + big-endian length + payload).
pub fn encode_frame(opcode: u8, payload: &[u8], buf: &mut Vec<u8>) {
    buf.clear();
    buf.push(opcode);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
}

/// A decoded frame.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// Read a single frame from `reader`. Returns `Ok(None)` on clean EOF.
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Frame>> {
    let mut header = [0u8; HEADER_LEN];
    let mut filled = 0;
    loop {
        match reader.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "session socket closed mid-frame",
                ));
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
        if filled == HEADER_LEN {
            break;
        }
    }
    let opcode = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(Some(Frame { opcode, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_round_trips() {
        let cases: &[(u8, &[u8])] = &[
            (OP_DATA, b"hello pty"),
            (OP_DETACH, b""),
            (OP_RESIZE, &[0, 24, 0, 80]),
            (OP_DATA, b""),
            (OP_DATA, &[0u8, 255, 254, 1]), // arbitrary binary must survive
        ];
        for (op, payload) in cases {
            let mut buf = Vec::new();
            encode_frame(*op, payload, &mut buf);
            let mut cursor = io::Cursor::new(buf);
            let frame = read_frame(&mut cursor).unwrap().unwrap();
            assert_eq!(frame.opcode, *op);
            assert_eq!(frame.payload.as_slice(), *payload);
        }
    }

    #[test]
    fn decode_clean_eof_is_none() {
        let mut cursor = io::Cursor::new(Vec::new());
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn decode_truncated_frame_errors() {
        let mut buf = Vec::new();
        encode_frame(OP_DATA, b"partial", &mut buf);
        buf.truncate(buf.len() - 2); // drop tail of payload
        let mut cursor = io::Cursor::new(buf);
        assert!(read_frame(&mut cursor).is_err());
    }
}
