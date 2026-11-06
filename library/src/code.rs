//! Explicit numeric mappings, independent of success and failure policies.

use crate::{Codec, EncodedSize, codec};

pub trait Code: Sized {
    type Repr: Copy + Eq;

    fn code(&self) -> Self::Repr;
    fn from_code(value: Self::Repr) -> Option<Self>;
}

/// Compose a numeric mapping with its representation's codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coded<T>(pub T);

impl<T> Codec for Coded<T>
where
    T: Code,
    T::Repr: Codec<Decoded = T::Repr>,
{
    type Decoded = Self;
    const SIZE: EncodedSize = T::Repr::SIZE;

    fn encode(&self, out: &mut [u8]) -> Option<usize> {
        codec::encode(&self.0.code(), out)
    }

    fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
        let (value, used) = codec::decode::<T::Repr>(bytes)?;
        Some((Self(T::from_code(value)?), used))
    }
}
