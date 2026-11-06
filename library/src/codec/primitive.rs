use super::Codec;
use crate::EncodedSize;

macro_rules! integers {
    ($($ty:ty),* $(,)?) => {$(
        impl Codec for $ty {
            type Decoded = Self;
            const SIZE: EncodedSize = EncodedSize::Fixed(core::mem::size_of::<Self>());

            fn encode(&self, out: &mut [u8]) -> Option<usize> {
                let bytes = self.to_le_bytes();
                out.get_mut(..bytes.len())?.copy_from_slice(&bytes);
                Some(bytes.len())
            }

            fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
                const WIDTH: usize = core::mem::size_of::<$ty>();
                let array = bytes.get(..WIDTH)?.try_into().ok()?;
                Some((Self::from_le_bytes(array), WIDTH))
            }
        }
    )*};
}

integers!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);

impl Codec for bool {
    type Decoded = Self;
    const SIZE: EncodedSize = EncodedSize::Fixed(1);

    fn encode(&self, out: &mut [u8]) -> Option<usize> {
        u8::from(*self).encode(out)
    }

    fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
        match bytes.first()? {
            0 => Some((false, 1)),
            1 => Some((true, 1)),
            _ => None,
        }
    }
}

impl Codec for () {
    type Decoded = Self;
    const SIZE: EncodedSize = EncodedSize::Fixed(0);

    fn encode(&self, _: &mut [u8]) -> Option<usize> {
        Some(0)
    }

    fn decode(_: &[u8]) -> Option<(Self, usize)> {
        Some(((), 0))
    }
}
