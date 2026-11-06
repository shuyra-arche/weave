//! Public variable-length composition mechanisms used by `Frame`.
//!
//! Prefixes encode byte counts for bytes/text and element counts for sequences.
//! Limits apply before allocating. All functions use relative consumed lengths.

use super::{Codec, decode, encode};
use crate::EncodedSize;

pub trait Length: Codec<Decoded = Self> + Copy {
    const MAX: u128;
    fn from_length(value: usize) -> Option<Self>;
    fn to_length(self) -> Option<usize>;
}

macro_rules! lengths {
    ($($ty:ty),* $(,)?) => {$(
        impl Length for $ty {
            const MAX: u128 = <$ty>::MAX as u128;
            fn from_length(value: usize) -> Option<Self> {
                Self::try_from(value).ok()
            }
            fn to_length(self) -> Option<usize> {
                usize::try_from(self).ok()
            }
        }
    )*};
}

lengths!(u8, u16, u32, u64, u128);

/// Compute a prefixed sequence's upper bound and validate prefix capacity.
pub const fn prefixed_size<P: Length>(element: EncodedSize, max: usize) -> EncodedSize {
    assert!(max as u128 <= P::MAX, "maximum exceeds length prefix range");
    EncodedSize::sum(&[P::SIZE, element.up_to(max)])
}

fn prefix<P: Length>(length: usize, max: usize, out: &mut [u8]) -> Option<usize> {
    if length > max || max as u128 > P::MAX {
        return None;
    }
    encode(&P::from_length(length)?, out)
}

fn count<P: Length>(bytes: &[u8], max: usize) -> Option<(usize, usize)> {
    if max as u128 > P::MAX {
        return None;
    }
    let (value, used) = decode::<P>(bytes)?;
    let length = value.to_length()?;
    (length <= max).then_some((length, used))
}

pub fn encode_bytes<P: Length>(value: &[u8], max: usize, out: &mut [u8]) -> Option<usize> {
    let used = prefix::<P>(value.len(), max, out)?;
    let end = used.checked_add(value.len())?;
    out.get_mut(used..end)?.copy_from_slice(value);
    Some(end)
}

/// Decode borrowed bytes; callers can choose their own ownership strategy.
pub fn decode_bytes<P: Length>(bytes: &[u8], max: usize) -> Option<(&[u8], usize)> {
    let (length, used) = count::<P>(bytes, max)?;
    let end = used.checked_add(length)?;
    Some((bytes.get(used..end)?, end))
}

pub fn encode_text<P: Length>(value: &str, max: usize, out: &mut [u8]) -> Option<usize> {
    encode_bytes::<P>(value.as_bytes(), max, out)
}

pub fn decode_text<P: Length>(bytes: &[u8], max: usize) -> Option<(&str, usize)> {
    let (value, used) = decode_bytes::<P>(bytes, max)?;
    Some((core::str::from_utf8(value).ok()?, used))
}

pub fn encode_sequence<P: Length, T: Codec>(
    values: &[T],
    max: usize,
    out: &mut [u8],
) -> Option<usize> {
    let mut used = prefix::<P>(values.len(), max, out)?;
    for value in values {
        used = used.checked_add(encode(value, out.get_mut(used..)?)?)?;
    }
    Some(used)
}

pub fn encode_counted<T: Codec, const N: usize>(
    values: &[T; N],
    count: usize,
    out: &mut [u8],
) -> Option<usize> {
    let mut used = 0usize;
    for value in values.get(..count)? {
        used = used.checked_add(encode(value, out.get_mut(used..)?)?)?;
    }
    Some(used)
}

pub fn decode_counted<T: Codec<Decoded = T>, const N: usize>(
    bytes: &[u8],
    count: usize,
    mut fill: impl FnMut() -> T,
) -> Option<([T; N], usize)> {
    if count > N {
        return None;
    }
    let mut values = core::array::from_fn::<_, N, _>(|_| None);
    let mut used = 0usize;
    for slot in &mut values[..count] {
        let (value, length) = decode::<T>(bytes.get(used..)?)?;
        used = used.checked_add(length)?;
        *slot = Some(value);
    }
    for slot in &mut values[count..] {
        *slot = Some(fill());
    }
    Some((
        values.map(|value| value.expect("all elements initialized")),
        used,
    ))
}

#[cfg(feature = "alloc")]
pub fn decode_owned_bytes<P: Length>(
    bytes: &[u8],
    max: usize,
) -> Option<(alloc::vec::Vec<u8>, usize)> {
    let (value, used) = decode_bytes::<P>(bytes, max)?;
    let mut owned = alloc::vec::Vec::new();
    owned.try_reserve_exact(value.len()).ok()?;
    owned.extend_from_slice(value);
    Some((owned, used))
}

#[cfg(feature = "alloc")]
pub fn decode_owned_text<P: Length>(
    bytes: &[u8],
    max: usize,
) -> Option<(alloc::string::String, usize)> {
    let (value, used) = decode_text::<P>(bytes, max)?;
    let mut owned = alloc::string::String::new();
    owned.try_reserve_exact(value.len()).ok()?;
    owned.push_str(value);
    Some((owned, used))
}

#[cfg(feature = "alloc")]
pub fn decode_sequence<P: Length, T: Codec>(
    bytes: &[u8],
    max: usize,
) -> Option<(alloc::vec::Vec<T::Decoded>, usize)> {
    let (length, mut used) = count::<P>(bytes, max)?;
    let mut values = alloc::vec::Vec::new();
    for _ in 0..length {
        let (value, consumed) = decode::<T>(bytes.get(used..)?)?;
        used = used.checked_add(consumed)?;
        values.try_reserve(1).ok()?;
        values.push(value);
    }
    Some((values, used))
}
