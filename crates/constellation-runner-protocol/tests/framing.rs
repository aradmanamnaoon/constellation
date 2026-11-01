use std::io::{self, Cursor, Read};

use constellation_runner_protocol::{read_frame, write_frame, MAX_WIRE_BYTES};

#[test]
fn bounded_roundtrip_preserves_multiple_frames_and_clean_eof() {
    let mut wire = Vec::new();
    write_frame(&mut wire, b"one").unwrap();
    write_frame(&mut wire, &vec![b'x'; MAX_WIRE_BYTES]).unwrap();
    let mut read = Cursor::new(wire);
    assert_eq!(read_frame(&mut read).unwrap(), Some(b"one".to_vec()));
    assert_eq!(
        read_frame(&mut read).unwrap(),
        Some(vec![b'x'; MAX_WIRE_BYTES])
    );
    assert_eq!(read_frame(&mut read).unwrap(), None);
}

#[test]
fn oversized_claim_is_rejected_before_reading_a_body() {
    for n in [0, MAX_WIRE_BYTES + 1, u32::MAX as usize] {
        let mut input = Cursor::new((n as u32).to_be_bytes());
        assert_eq!(
            read_frame(&mut input).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    let mut output = Vec::new();
    assert_eq!(
        write_frame(&mut output, &vec![0; MAX_WIRE_BYTES + 1])
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(output.is_empty());
}

#[test]
fn truncated_header_and_body_fail_closed() {
    for bytes in [&[0_u8][..], &[0, 0, 0][..], &[0, 0, 0, 3, b'a'][..]] {
        assert_eq!(
            read_frame(&mut Cursor::new(bytes)).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

struct OneByteReader<'a>(&'a [u8]);
impl Read for OneByteReader<'_> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() || into.is_empty() {
            return Ok(0);
        }
        into[0] = self.0[0];
        self.0 = &self.0[1..];
        Ok(1)
    }
}

#[test]
fn partial_transport_reads_do_not_shift_frame_boundaries() {
    let mut wire = Vec::new();
    write_frame(&mut wire, b"abc").unwrap();
    write_frame(&mut wire, b"xyz").unwrap();
    let mut reader = OneByteReader(&wire);
    assert_eq!(
        read_frame(&mut reader).unwrap().as_deref(),
        Some(&b"abc"[..])
    );
    assert_eq!(
        read_frame(&mut reader).unwrap().as_deref(),
        Some(&b"xyz"[..])
    );
    assert_eq!(read_frame(&mut reader).unwrap(), None);
}
