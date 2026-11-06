//! Prefix codecs and checked composition helpers.

pub mod bounded;
mod primitive;
mod sequence;

use crate::EncodedSize;

pub trait Codec: Sized {
    type Decoded;

    const SIZE: EncodedSize;

    /// Encode at offset zero and return bytes written.
    /// Failure may leave partial writes; callers must discard that output.
    fn encode(&self, out: &mut [u8]) -> Option<usize>;

    /// Decode one prefix and return its value and consumed byte count.
    fn decode(bytes: &[u8]) -> Option<(Self::Decoded, usize)>;

    /// Decode one complete representation, rejecting trailing bytes.
    fn decode_exact(bytes: &[u8]) -> Option<Self::Decoded> {
        let (value, used) = decode::<Self>(bytes)?;
        (used == bytes.len()).then_some(value)
    }
}

/// Restrict available storage to the codec's declared maximum.
pub fn output(size: EncodedSize, out: &mut [u8]) -> &mut [u8] {
    let limit = size.maximum().unwrap_or(out.len()).min(out.len());
    &mut out[..limit]
}

/// Restrict readable bytes to the codec's declared maximum.
pub fn input(size: EncodedSize, bytes: &[u8]) -> &[u8] {
    let limit = size.maximum().unwrap_or(bytes.len()).min(bytes.len());
    &bytes[..limit]
}

/// Encode a child codec while checking its reported length and size guarantee.
pub fn encode<T: Codec>(value: &T, out: &mut [u8]) -> Option<usize> {
    let out = output(T::SIZE, out);
    let used = value.encode(out)?;
    (used <= out.len() && T::SIZE.accepts(used)).then_some(used)
}

/// Decode a child codec while checking its reported length and size guarantee.
pub fn decode<T: Codec>(bytes: &[u8]) -> Option<(T::Decoded, usize)> {
    let bytes = input(T::SIZE, bytes);
    let (value, used) = T::decode(bytes)?;
    (used <= bytes.len() && T::SIZE.accepts(used)).then_some((value, used))
}
