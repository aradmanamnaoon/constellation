//! Length-delimited transport framing, with no socket or process operations.
//! Framing is not authentication: callers MUST verify the HMAC signed body.
use std::io::{self, Read, Write};

use crate::MAX_WIRE_BYTES;

/// Reads one bounded big-endian u32 length-prefixed message. EOF is only
/// acceptable *between* frames, never during a header or payload.
pub fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0_u8; 4];
    match input.read(&mut prefix[..1])? {
        0 => return Ok(None),
        1 => {}
        _ => unreachable!(),
    }
    input.read_exact(&mut prefix[1..])?;
    let len = u32::from_be_bytes(prefix) as usize;
    if len == 0 || len > MAX_WIRE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "runner frame length exceeds limit",
        ));
    }
    let mut body = vec![0; len];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_frame(output: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.is_empty() || body.len() > MAX_WIRE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runner frame length exceeds limit",
        ));
    }
    output.write_all(&(body.len() as u32).to_be_bytes())?;
    output.write_all(body)?;
    output.flush()
}
