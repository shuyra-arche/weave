#![cfg(all(feature = "derive", feature = "alloc"))]

use weave::{Codec, EncodedSize};

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Packet {
    version: u8,
    #[frame(bytes(prefix = u16, max = 4096))]
    payload: Vec<u8>,
    checksum: u32,
}

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Entry {
    #[frame(text(prefix = u8, max = 12))]
    name: String,
}

#[derive(Debug, PartialEq, Eq, weave::Frame)]
#[frame(max = 20)]
struct Batch {
    #[frame(sequence(prefix = u16, max = 4))]
    entries: Vec<Entry>,
}

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Envelope {
    batch: Batch,
    tail: u64,
}

#[test]
fn variable_bytes_use_a_prefix_and_preserve_following_fields() {
    let packet = Packet {
        version: 1,
        payload: vec![1, 2, 3, 4, 5],
        checksum: 0x12345678,
    };
    assert_eq!(Packet::SIZE, EncodedSize::Bounded(4103));
    let mut out = [0; 12];
    assert_eq!(packet.encode(&mut out), Some(12));
    assert_eq!(out, [1, 5, 0, 1, 2, 3, 4, 5, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(Packet::decode_exact(&out), Some(packet));
    for end in 0..out.len() {
        assert!(Packet::decode(&out[..end]).is_none());
    }
    assert!(Packet::decode(&[1, 1, 16]).is_none()); // declared 4097 bytes
}

#[test]
fn nested_variable_frames_consume_only_their_own_prefix() {
    let envelope = Envelope {
        batch: Batch {
            entries: vec![
                Entry { name: "hi".into() },
                Entry {
                    name: "世界".into(),
                },
            ],
        },
        tail: 42,
    };
    let mut out = [0; 128];
    let used = envelope.encode(&mut out).unwrap();
    assert_eq!(used, 2 + 3 + 7 + 8);
    assert_eq!(Envelope::decode_exact(&out[..used]), Some(envelope));
    assert!(Envelope::decode_exact(&out).is_none());
}

#[test]
fn whole_frame_limit_is_enforced_even_with_large_buffers() {
    assert_eq!(Batch::SIZE, EncodedSize::Bounded(20));
    let oversized = Batch {
        entries: vec![
            Entry {
                name: "a".repeat(12),
            },
            Entry {
                name: "b".repeat(12),
            },
        ],
    };
    assert!(oversized.encode(&mut [0; 1024]).is_none());
    let mut bytes = vec![2, 0, 12];
    bytes.extend_from_slice(&[b'a'; 12]);
    bytes.push(12);
    bytes.extend_from_slice(&[b'b'; 12]);
    assert!(Batch::decode(&bytes).is_none());
    assert!(Batch::decode(&[5, 0]).is_none());
}

#[test]
fn utf8_and_byte_limits_are_checked() {
    let valid = Entry {
        name: "世界".into(),
    };
    let mut out = [0; 32];
    let used = valid.encode(&mut out).unwrap();
    assert_eq!(out[0], 6);
    assert_eq!(Entry::decode_exact(&out[..used]), Some(valid));
    assert!(Entry::decode(&[1, 0xff]).is_none());
    assert!(
        Entry {
            name: "世".repeat(5)
        }
        .encode(&mut out)
        .is_none()
    );
    assert!(Entry::decode(&[13]).is_none());
}

#[test]
fn all_successful_packet_prefixes_round_trip_with_exact_lengths() {
    for length in [0, 1, 255, 256, 4096] {
        let packet = Packet {
            version: 1,
            payload: vec![0x55; length],
            checksum: 99,
        };
        let mut out = vec![0; length + 7];
        let used = packet.encode(&mut out).unwrap();
        assert_eq!(used, length + 7);
        assert_eq!(Packet::decode_exact(&out), Some(packet));
    }
    let oversized = Packet {
        version: 1,
        payload: vec![0; 4097],
        checksum: 0,
    };
    assert!(oversized.encode(&mut vec![0; 8192]).is_none());
}
