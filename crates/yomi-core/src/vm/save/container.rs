//! The outer frame of the bytes of saves and persistent data (marker, version, CRC-32).

use super::*;

/// CRC-32 (IEEE 802.3).
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Makes bytes with the magic, version, body and CRC-32 in a row.
pub(in crate::vm) fn wrap(magic: &[u8; 4], version: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(magic);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(body);
    let crc = crc32(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Checks the magic, version and checksum, and returns the body part.
pub(in crate::vm) fn unwrap<'b>(
    bytes: &'b [u8],
    magic: &[u8; 4],
    version: u32,
) -> Result<&'b [u8], LoadError> {
    if bytes.len() < 12 || &bytes[..4] != magic {
        return fail(LoadErrorKind::BadMagic);
    }
    let found = u32::from_le_bytes(bytes[4..8].try_into().expect("4 バイト"));
    if found != version {
        return fail(LoadErrorKind::UnsupportedVersion(found));
    }
    let (head, crc_bytes) = bytes.split_at(bytes.len() - 4);
    let crc = u32::from_le_bytes(crc_bytes.try_into().expect("4 バイト"));
    if crc32(head) != crc {
        return fail(LoadErrorKind::ChecksumMismatch);
    }
    Ok(&head[8..])
}
