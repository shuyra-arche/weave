#![cfg(feature = "derive")]

use weave::{Code, Codec, Contract, EncodedSize};

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Header {
    version: u8,
    sequence: u32,
}

#[derive(Debug, PartialEq, Eq, weave::Frame)]
#[frame(max = 16)]
struct Generic<T, const N: usize> {
    values: [T; N],
}

#[derive(Debug, PartialEq, Eq, weave::Code)]
#[repr(u8)]
enum Operation {
    Read = 1,
    Write = 7,
}

#[derive(Debug, PartialEq, Eq, weave::Code)]
#[repr(i32)]
enum Failure {
    Denied = -1,
    Busy = -3,
}

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Request {
    #[frame(code)]
    operation: Operation,
    offset: u64,
}

#[derive(weave::Contract)]
#[contract(request = Request, response = Header)]
struct Read;

#[derive(Debug, PartialEq, Eq, weave::Frame)]
struct Page {
    count: u16,
    #[frame(sequence(count = count, fill = 0xffff))]
    entries: [u16; 3],
    checksum: u8,
}

#[test]
fn fixed_frames_preserve_field_order_and_report_exact_lengths() {
    let value = Header {
        version: 2,
        sequence: 0x12345678,
    };
    let mut out = [0; 32];
    assert_eq!(Header::SIZE, EncodedSize::Fixed(5));
    assert_eq!(value.encode(&mut out), Some(5));
    assert_eq!(&out[..5], &[2, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(Header::decode_exact(&out[..5]), Some(value));
    assert!(Header::decode_exact(&out).is_none());
    assert!(Header::decode(&out[..4]).is_none());
}

#[test]
fn generics_arrays_and_frame_limits_compose() {
    let value = Generic {
        values: [1u16, 2, 3],
    };
    assert_eq!(Generic::<u16, 3>::SIZE, EncodedSize::Fixed(6));
    let mut out = [0; 6];
    assert_eq!(value.encode(&mut out), Some(6));
    assert_eq!(Generic::<u16, 3>::decode_exact(&out), Some(value));
}

#[test]
fn codes_and_contracts_are_usable_through_public_exports() {
    assert_eq!(Operation::Write.code(), 7);
    assert_eq!(Operation::from_code(3), None);
    assert_eq!(Failure::Busy.code(), -3);
    assert_eq!(Failure::from_code(-2), None);
    assert_eq!(Request::SIZE, EncodedSize::Fixed(9));
    let request: <Read as Contract>::Request = Request {
        operation: Operation::Read,
        offset: 9,
    };
    let mut out = [0; 9];
    request.encode(&mut out).unwrap();
    assert_eq!(Request::decode_exact(&out), Some(request));
    out[0] = 3;
    assert!(Request::decode(&out).is_none());
}

#[test]
fn counted_arrays_fill_unused_slots_and_leave_following_fields_intact() {
    let page = Page {
        count: 2,
        entries: [10, 20, 999],
        checksum: 77,
    };
    assert_eq!(Page::SIZE, EncodedSize::Bounded(9));
    let mut out = [0; 32];
    let used = page.encode(&mut out).unwrap();
    assert_eq!(used, 7);
    assert_eq!(
        Page::decode_exact(&out[..used]),
        Some(Page {
            count: 2,
            entries: [10, 20, 0xffff],
            checksum: 77
        })
    );
    let overflow = Page { count: 4, ..page };
    assert!(overflow.encode(&mut out).is_none());
    assert!(Page::decode(&[4, 0]).is_none());
    assert!(Page::decode(&[255, 255]).is_none());
}

#[test]
fn explicit_library_path_supports_a_renamed_import() {
    use weave as protocol;
    #[derive(protocol::Code)]
    #[weave(crate = protocol)]
    #[repr(u16)]
    enum Tag {
        Value = 1000,
    }
    #[derive(protocol::Frame)]
    #[weave(crate = protocol)]
    struct Value {
        #[frame(code)]
        tag: Tag,
    }
    #[derive(protocol::Contract)]
    #[weave(crate = protocol)]
    #[contract(request = Value, response = Value)]
    struct Echo;
    let value: <Echo as Contract>::Request = Value { tag: Tag::Value };
    let mut out = [0; 2];
    assert_eq!(value.encode(&mut out), Some(2));
    assert_eq!(out, [0xe8, 3]);
}
