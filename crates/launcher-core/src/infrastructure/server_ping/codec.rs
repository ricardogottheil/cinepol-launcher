//! Wire format of the Server List Ping: VarInts, strings and length-prefixed packets.

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Packets longer than this are refused before anything is allocated.
pub(crate) const MAX_PACKET_LEN: usize = 2 * 1024 * 1024;
const MAX_VARINT_BYTES: usize = 5;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum FrameError {
    #[error("a VarInt is longer than 5 bytes")]
    VarIntTooLong,
    #[error("the packet length {0} is out of range")]
    BadLength(i64),
    #[error("the packet ended early")]
    Truncated,
    #[error("the packet is malformed: {0}")]
    Malformed(&'static str),
    #[error("connection error: {0}")]
    Io(String),
}

impl From<std::io::Error> for FrameError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(error.to_string())
        }
    }
}

pub(crate) fn encode_varint(value: i32, out: &mut Vec<u8>) {
    // VarInts encode the two's complement bits, so negatives take 5 bytes.
    let mut rest = value as u32;
    loop {
        let byte = (rest & 0x7f) as u8;
        rest >>= 7;
        if rest == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Decodes a VarInt at the start of `bytes`, returning it and its length.
pub(crate) fn decode_varint(bytes: &[u8]) -> Result<(i32, usize), FrameError> {
    let mut value: u32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if index == MAX_VARINT_BYTES {
            return Err(FrameError::VarIntTooLong);
        }
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((value as i32, index + 1));
        }
    }
    if bytes.len() >= MAX_VARINT_BYTES {
        Err(FrameError::VarIntTooLong)
    } else {
        Err(FrameError::Truncated)
    }
}

pub(crate) async fn read_varint<R: AsyncRead + Unpin>(reader: &mut R) -> Result<i32, FrameError> {
    let mut buffer = [0u8; MAX_VARINT_BYTES];
    for index in 0..MAX_VARINT_BYTES {
        let byte = reader.read_u8().await?;
        buffer[index] = byte;
        if byte & 0x80 == 0 {
            return decode_varint(&buffer[..=index]).map(|(value, _)| value);
        }
    }
    Err(FrameError::VarIntTooLong)
}

pub(crate) fn encode_string(value: &str, out: &mut Vec<u8>) {
    let len = i32::try_from(value.len()).unwrap_or(i32::MAX);
    encode_varint(len, out);
    out.extend_from_slice(value.as_bytes());
}

/// Decodes a VarInt-prefixed UTF-8 string at the start of `bytes`.
pub(crate) fn decode_string(bytes: &[u8]) -> Result<(String, usize), FrameError> {
    let (len, prefix) = decode_varint(bytes)?;
    let len = usize::try_from(len).map_err(|_| FrameError::BadLength(i64::from(len)))?;
    let end = prefix.checked_add(len).ok_or(FrameError::Truncated)?;
    let raw = bytes.get(prefix..end).ok_or(FrameError::Truncated)?;
    let text = std::str::from_utf8(raw).map_err(|_| FrameError::Malformed("invalid UTF-8"))?;
    Ok((text.to_owned(), end))
}

/// Wraps `payload` in a packet: length, packet id, payload.
pub(crate) fn frame(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + MAX_VARINT_BYTES);
    encode_varint(id, &mut body);
    body.extend_from_slice(payload);
    let mut packet = Vec::with_capacity(body.len() + MAX_VARINT_BYTES);
    encode_varint(i32::try_from(body.len()).unwrap_or(i32::MAX), &mut packet);
    packet.extend_from_slice(&body);
    packet
}

/// Reads one packet and returns its id and the bytes after the id.
pub(crate) async fn read_packet<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<(i32, Vec<u8>), FrameError> {
    let len = read_varint(reader).await?;
    let len = usize::try_from(len)
        .ok()
        .filter(|len| (1..=MAX_PACKET_LEN).contains(len))
        .ok_or(FrameError::BadLength(i64::from(len)))?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let (id, id_len) = decode_varint(&body)?;
    Ok((id, body.split_off(id_len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        encode_varint(value, &mut out);
        out
    }

    #[test]
    fn varints_round_trip() {
        for value in [
            0,
            1,
            127,
            128,
            255,
            25565,
            2_097_151,
            -1,
            i32::MAX,
            i32::MIN,
        ] {
            let bytes = encoded(value);
            assert_eq!(decode_varint(&bytes), Ok((value, bytes.len())), "{value}");
        }
        assert_eq!(encoded(-1), [0xff, 0xff, 0xff, 0xff, 0x0f]);
        assert_eq!(encoded(i32::MAX), [0xff, 0xff, 0xff, 0xff, 0x07]);
        assert_eq!(encoded(300), [0xac, 0x02]);
    }

    #[test]
    fn rejects_a_six_byte_varint() {
        let bytes = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(decode_varint(&bytes), Err(FrameError::VarIntTooLong));
    }

    #[tokio::test]
    async fn reading_rejects_a_six_byte_varint() {
        let bytes: &[u8] = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        let mut reader = bytes;
        assert_eq!(
            read_varint(&mut reader).await,
            Err(FrameError::VarIntTooLong)
        );
    }

    #[tokio::test]
    async fn rejects_an_oversized_packet_length_before_reading_it() {
        let mut bytes = Vec::new();
        encode_varint(i32::try_from(MAX_PACKET_LEN + 1).unwrap(), &mut bytes);
        let mut reader = bytes.as_slice();
        assert!(matches!(
            read_packet(&mut reader).await,
            Err(FrameError::BadLength(_))
        ));
    }

    #[tokio::test]
    async fn rejects_negative_and_empty_packet_lengths() {
        for len in [-1, 0] {
            let mut bytes = Vec::new();
            encode_varint(len, &mut bytes);
            let mut reader = bytes.as_slice();
            assert!(matches!(
                read_packet(&mut reader).await,
                Err(FrameError::BadLength(_))
            ));
        }
    }

    #[tokio::test]
    async fn frames_round_trip() {
        let mut payload = Vec::new();
        encode_string("hello", &mut payload);
        let packet = frame(0x00, &payload);
        let mut reader = packet.as_slice();
        let (id, body) = read_packet(&mut reader).await.unwrap();
        assert_eq!(id, 0x00);
        assert_eq!(decode_string(&body), Ok(("hello".to_owned(), 6)));
    }

    #[tokio::test]
    async fn a_truncated_packet_is_reported() {
        let packet = frame(0x00, b"abcdef");
        let mut reader = &packet[..4];
        assert_eq!(read_packet(&mut reader).await, Err(FrameError::Truncated));
    }

    #[test]
    fn a_string_longer_than_its_packet_is_rejected() {
        let mut bytes = Vec::new();
        encode_varint(100, &mut bytes);
        bytes.extend_from_slice(b"short");
        assert_eq!(decode_string(&bytes), Err(FrameError::Truncated));
    }
}
