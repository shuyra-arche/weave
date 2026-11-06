use super::{Codec, decode, encode, input, output};
use crate::EncodedSize;

impl<T: Codec, const N: usize> Codec for [T; N] {
    type Decoded = [T::Decoded; N];
    const SIZE: EncodedSize = T::SIZE.repeated(N);

    fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let out = output(Self::SIZE, out);
        let mut used = 0usize;
        for value in self {
            used = used.checked_add(encode(value, out.get_mut(used..)?)?)?;
        }
        Some(used)
    }

    fn decode(bytes: &[u8]) -> Option<(Self::Decoded, usize)> {
        let bytes = input(Self::SIZE, bytes);
        let mut values = core::array::from_fn::<_, N, _>(|_| None);
        let mut used = 0usize;
        for slot in &mut values {
            let (value, length) = decode::<T>(bytes.get(used..)?)?;
            used = used.checked_add(length)?;
            *slot = Some(value);
        }
        Some((
            values.map(|value| value.expect("all elements decoded")),
            used,
        ))
    }
}
