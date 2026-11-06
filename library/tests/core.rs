use weave::{Code, Codec, Coded, EncodedSize, codec};

#[derive(Debug, PartialEq, Eq)]
enum Operation {
    Read,
    Write,
}

impl Code for Operation {
    type Repr = u8;

    fn code(&self) -> u8 {
        match self {
            Self::Read => 1,
            Self::Write => 7,
        }
    }

    fn from_code(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Read),
            7 => Some(Self::Write),
            _ => None,
        }
    }
}

#[test]
fn integers_use_little_endian_and_exact_decode_rejects_trailing_data() {
    let mut out = [0xaa; 8];
    assert_eq!(0x1234u16.encode(&mut out), Some(2));
    assert_eq!(out, [0x34, 0x12, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa]);
    assert_eq!(u16::decode(&out), Some((0x1234, 2)));
    assert_eq!(u16::decode_exact(&out), None);
    assert_eq!(u16::decode_exact(&out[..2]), Some(0x1234));
    assert_eq!(u16::decode(&out[..1]), None);
    assert_eq!(bool::decode(&[2]), None);
    assert_eq!(<()>::decode_exact(&[]), Some(()));
}

#[test]
fn numeric_mapping_composes_without_copy_requirement() {
    let value = Coded(Operation::Write);
    let mut out = [0; 1];
    assert_eq!(value.encode(&mut out), Some(1));
    assert_eq!(out, [7]);
    assert_eq!(Coded::<Operation>::decode_exact(&out), Some(value));
    assert_eq!(Coded::<Operation>::decode(&[3]), None);
}

#[test]
fn bounded_mechanisms_are_usable_without_macros_or_alloc() {
    use codec::bounded::*;
    let mut out = [0; 32];
    let length = encode_bytes::<u16>(&[1, 2, 3], 8, &mut out).unwrap();
    assert_eq!(&out[..length], &[3, 0, 1, 2, 3]);
    assert_eq!(
        decode_bytes::<u16>(&out[..length], 8),
        Some((&[1, 2, 3][..], 5))
    );
    assert_eq!(encode_bytes::<u8>(&[0; 9], 8, &mut out), None);
    assert_eq!(decode_bytes::<u8>(&[9, 0], 8), None);
    assert_eq!(decode_bytes::<u16>(&[3, 0, 1], 8), None);
    assert_eq!(decode_text::<u8>(&[1, 0xff], 8), None);
    assert_eq!(
        decode_bytes::<u128>(&u128::MAX.to_le_bytes(), usize::MAX),
        None
    );
}

#[test]
fn sizes_distinguish_exact_lengths_from_bounds() {
    use EncodedSize::*;
    assert_eq!(EncodedSize::sum(&[Fixed(1), Fixed(2)]), Fixed(3));
    assert_eq!(EncodedSize::sum(&[Fixed(1), Bounded(2)]), Bounded(3));
    assert_eq!(EncodedSize::sum(&[Fixed(1), Unbounded]), Unbounded);
    assert_eq!(Fixed(3).limited(10), Fixed(3));
    assert_eq!(Bounded(100).limited(10), Bounded(10));
    assert_eq!(Unbounded.limited(10), Bounded(10));
    assert_eq!(Unbounded.repeated(0), Fixed(0));
    assert_eq!(Fixed(12).up_to(32), Bounded(384));
    assert_eq!(EncodedSize::choice(&[Fixed(3), Fixed(3)]), Fixed(3));
    assert_eq!(EncodedSize::choice(&[Fixed(3), Fixed(5)]), Bounded(5));
    assert_eq!(EncodedSize::choice(&[Bounded(3), Fixed(3)]), Bounded(3));
    assert_eq!(EncodedSize::choice(&[Fixed(3), Unbounded]), Unbounded);
    assert_eq!(EncodedSize::choice(&[Fixed(0)]), Fixed(0));
}

#[test]
#[should_panic(expected = "encoded size addition overflow")]
fn size_overflow_is_not_silently_unbounded() {
    std::hint::black_box(EncodedSize::sum(&[
        EncodedSize::Fixed(usize::MAX),
        EncodedSize::Fixed(1),
    ]));
}

#[test]
fn checked_composition_rejects_invalid_consumption_reports() {
    struct Invalid;
    impl Codec for Invalid {
        type Decoded = Self;
        const SIZE: EncodedSize = EncodedSize::Fixed(1);
        fn encode(&self, _: &mut [u8]) -> Option<usize> {
            Some(2)
        }
        fn decode(_: &[u8]) -> Option<(Self, usize)> {
            Some((Self, 2))
        }
    }
    assert_eq!(codec::encode(&Invalid, &mut [0; 8]), None);
    assert!(codec::decode::<Invalid>(&[0; 8]).is_none());
    assert!(Invalid::decode_exact(&[0; 8]).is_none());
}

#[test]
fn array_decode_drops_previously_decoded_values_on_failure() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Tracked;
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Codec for Tracked {
        type Decoded = Self;
        const SIZE: EncodedSize = EncodedSize::Fixed(1);
        fn encode(&self, out: &mut [u8]) -> Option<usize> {
            1u8.encode(out)
        }
        fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
            (*bytes.first()? == 1).then_some((Self, 1))
        }
    }
    DROPS.store(0, Ordering::SeqCst);
    assert!(<[Tracked; 3]>::decode(&[1, 1]).is_none());
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}
