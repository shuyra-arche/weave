#![cfg(feature = "alloc")]

use std::sync::atomic::{AtomicUsize, Ordering};
use weave::{Codec, EncodedSize, codec::bounded};

#[test]
fn zero_width_elements_still_obey_the_element_count_limit() {
    static DECODES: AtomicUsize = AtomicUsize::new(0);

    struct Empty;
    impl Codec for Empty {
        type Decoded = ();
        const SIZE: EncodedSize = EncodedSize::Fixed(0);

        fn encode(&self, _: &mut [u8]) -> Option<usize> {
            Some(0)
        }

        fn decode(_: &[u8]) -> Option<((), usize)> {
            DECODES.fetch_add(1, Ordering::Relaxed);
            Some(((), 0))
        }
    }

    assert_eq!(
        bounded::prefixed_size::<u8>(Empty::SIZE, 4),
        EncodedSize::Fixed(1)
    );
    assert!(bounded::decode_sequence::<u8, Empty>(&[5], 4).is_none());
    assert_eq!(DECODES.load(Ordering::Relaxed), 0);
    assert_eq!(
        bounded::decode_sequence::<u8, Empty>(&[4, 99], 4),
        Some((vec![(); 4], 1))
    );
    assert_eq!(DECODES.load(Ordering::Relaxed), 4);

    let mut bytes = [0; 1];
    assert_eq!(
        bounded::encode_sequence::<u8, _>(&[(); 4], 4, &mut bytes),
        Some(1)
    );
    assert_eq!(bytes, [4]);
    assert!(bounded::encode_sequence::<u8, _>(&[(); 5], 4, &mut bytes).is_none());
}

#[test]
fn owned_sequence_drops_all_decoded_values_if_a_later_element_fails() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);

    struct Tracked;
    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl Codec for Tracked {
        type Decoded = Self;
        const SIZE: EncodedSize = EncodedSize::Fixed(1);

        fn encode(&self, out: &mut [u8]) -> Option<usize> {
            1u8.encode(out)
        }

        fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
            if *bytes.first()? == 1 {
                Some((Self, 1))
            } else {
                None
            }
        }
    }

    assert!(bounded::decode_sequence::<u8, Tracked>(&[3, 1, 1], 3).is_none());
    assert_eq!(DROPS.load(Ordering::Relaxed), 2);
}
