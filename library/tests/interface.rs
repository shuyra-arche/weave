#![cfg(feature = "derive")]

use weave::{Code, Codec, Contract, EncodedSize};

#[test]
fn operation_adapters_preserve_distinct_decoded_types_and_reject_other_branches() {
    use weave::Operation;
    let request = storage::Write::wrap_request(Number(23));
    assert!(matches!(request, storage::Request::Write(Number(23))));
    let response = storage::Response::decode_exact(&[1, 0, 99, 0]).unwrap();
    assert_eq!(storage::Read::unwrap_response(response), Some(99u16));
    assert_eq!(
        storage::Read::unwrap_response(storage::DecodedResponse::Write(1)),
        None
    );
}

pub struct Number(pub u16);

impl Codec for Number {
    type Decoded = u16;
    const SIZE: EncodedSize = EncodedSize::Fixed(2);
    fn encode(&self, out: &mut [u8]) -> Option<usize> {
        self.0.encode(out)
    }
    fn decode(bytes: &[u8]) -> Option<(u16, usize)> {
        u16::decode(bytes)
    }
}

#[weave::interface(tag = u16, dispatch)]
pub mod storage {
    use super::Number;

    pub const DESCRIPTION: &str = "ordinary declarations are preserved";

    #[operation(code = 1, request = u32, response = Number, handler = read)]
    #[cfg(feature = "derive")]
    pub struct Read;

    #[operation(code = 7, request = Number, response = u8, handler = write)]
    pub struct Write;

    #[cfg(any())]
    #[operation(code = 9, request = Missing, response = Missing, handler = missing)]
    pub struct Disabled;

    #[cfg_attr(all(), cfg(any()), derive(Clone))]
    #[operation(code = 10, request = Missing, response = Missing, handler = hidden)]
    pub struct Hidden;

    #[cfg_attr(all(), cfg_attr(all(), cfg(any())))]
    #[operation(code = 11, request = Missing, response = Missing, handler = nested)]
    pub struct Nested;
}

struct Service {
    calls: usize,
}

impl storage::Handler for Service {
    type Error = &'static str;
    fn read(&mut self, request: u32) -> Result<Number, Self::Error> {
        self.calls += 1;
        Ok(Number(u16::try_from(request).map_err(|_| "overflow")?))
    }
    fn write(&mut self, request: u16) -> Result<u8, Self::Error> {
        self.calls += 1;
        u8::try_from(request).map_err(|_| "overflow")
    }
}

#[test]
fn tags_contracts_and_request_response_codecs_share_one_declaration() {
    assert_eq!(storage::Read::TAG, storage::Operation::Read);
    assert_eq!(storage::Write::TAG.code(), 7);
    assert_eq!(storage::Operation::from_code(2), None);
    assert!(!storage::DESCRIPTION.is_empty());
    let request: <storage::Write as Contract>::Request = Number(0x1234);
    let mut out = [0xaa; 8];
    let length = storage::Request::Write(request).encode(&mut out).unwrap();
    assert_eq!(length, 4);
    assert_eq!(&out[..length], &[7, 0, 0x34, 0x12]);
    assert_eq!(&out[length..], &[0xaa; 4]);
    assert!(matches!(
        storage::Request::decode_exact(&out[..length]),
        Some(storage::DecodedRequest::Write(0x1234))
    ));

    let response: <storage::Read as Contract>::Response = Number(99);
    let length = storage::Response::Read(response).encode(&mut out).unwrap();
    assert_eq!(&out[..length], &[1, 0, 99, 0]);
    assert!(matches!(
        storage::Response::decode_exact(&out[..length]),
        Some(storage::DecodedResponse::Read(99))
    ));
}

#[test]
fn malformed_tags_truncation_and_trailing_bytes_are_rejected() {
    assert!(storage::Request::decode(&[2, 0, 1, 0]).is_none());
    assert!(storage::Request::decode(&[7]).is_none());
    assert!(storage::Request::decode(&[7, 0, 1]).is_none());
    assert!(storage::Response::decode(&[2, 0, 1, 0]).is_none());
    let bytes = [7, 0, 1, 0, 255];
    assert!(matches!(
        storage::Request::decode(&bytes),
        Some((storage::DecodedRequest::Write(1), 4))
    ));
    assert!(storage::Request::decode_exact(&bytes).is_none());
    assert!(
        storage::Request::Write(Number(1))
            .encode(&mut [0; 3])
            .is_none()
    );
}

#[test]
fn dispatch_receives_decoded_values_and_propagates_local_errors() {
    let mut service = Service { calls: 0 };
    let request = storage::Request::decode_exact(&[7, 0, 42, 0]).unwrap();
    let response = storage::dispatch(&mut service, request).unwrap();
    assert!(matches!(response, storage::Response::Write(42)));
    let error = storage::dispatch(&mut service, storage::DecodedRequest::Write(256));
    assert!(matches!(error, Err("overflow")));
    assert_eq!(service.calls, 2);
    let handler: &mut dyn storage::Handler<Error = &'static str> = &mut service;
    assert!(storage::dispatch(handler, storage::DecodedRequest::Read(3)).is_ok());
}

#[test]
fn conditional_operations_are_removed_from_all_generated_surfaces() {
    assert_eq!(storage::Operation::from_code(9), None);
    assert_eq!(storage::Operation::from_code(10), None);
    assert_eq!(storage::Operation::from_code(11), None);
    assert_eq!(storage::Request::SIZE, EncodedSize::Bounded(6));
    assert_eq!(storage::Response::SIZE, EncodedSize::Bounded(4));
}

#[weave::interface(tag = u8)]
mod equal {
    #[operation(code = 0, request = u16, response = ())]
    pub struct First;
    #[operation(code = 255, request = u16, response = ())]
    pub struct Second;
}

#[test]
fn identical_fixed_branches_remain_fixed_and_zero_codes_are_allowed() {
    assert_eq!(equal::Request::SIZE, EncodedSize::Fixed(3));
    assert_eq!(equal::Response::SIZE, EncodedSize::Fixed(1));
    let mut out = [0; 3];
    equal::Request::First(7).encode(&mut out).unwrap();
    assert_eq!(out, [0, 7, 0]);
    assert!(matches!(
        equal::Request::decode_exact(&out),
        Some(equal::DecodedRequest::First(7))
    ));
    assert_eq!(equal::First::TAG.code(), 0);
    assert_eq!(equal::Second::TAG.code(), 255);
    assert_eq!(equal::Response::Second(()).encode(&mut out), Some(1));
}

#[weave::interface(tag = u128)]
#[weave(crate = ::weave)]
mod wide {
    // These names must not conflict with Codec::Decoded or Code::Repr.
    #[operation(code = 340282366920938463463374607431768211455, request = (), response = ())]
    pub struct Decoded;
    #[operation(code = 0, request = (), response = ())]
    pub struct Repr;
}

#[test]
fn maximum_tag_and_associated_type_like_operation_names_work() {
    let mut out = [0; 16];
    assert_eq!(wide::Request::Decoded(()).encode(&mut out), Some(16));
    assert_eq!(out, [255; 16]);
    assert_eq!(wide::Decoded::TAG.code(), u128::MAX);
    assert_eq!(wide::Repr::TAG.code(), 0);
    assert!(matches!(
        wide::Request::decode_exact(&out),
        Some(wide::DecodedRequest::Decoded(()))
    ));
}

use weave as renamed;

#[renamed::interface(tag = u8, client)]
#[weave(crate = crate::renamed)]
mod client_only {
    use super::Number;
    // Operation names must not be shadowed by generated generic parameters.
    #[operation(code = 1, request = Number, response = Number, call = r#type)]
    pub struct S;
    #[operation(code = 2, request = (), response = (), call = ping)]
    pub struct R;
    #[cfg(any())]
    #[operation(code = 3, request = Missing, response = Missing, call = missing)]
    pub struct Disabled;
    #[cfg_attr(all(), cfg_attr(all(), cfg(any())))]
    #[operation(code = 4, request = Missing, response = Missing, call = nested)]
    pub struct Hidden;
}

#[test]
fn client_only_generation_supports_aliases_raw_names_and_distinct_decoded_types() {
    struct Sender(Vec<Vec<u8>>);
    impl weave::FrameSender for Sender {
        type Error = ();
        fn send(&mut self, bytes: &[u8]) -> Result<(), ()> {
            self.0.push(bytes.to_vec());
            Ok(())
        }
    }
    struct Reply(&'static [u8]);
    impl weave::FrameReceiver for Reply {
        type Error = ();
        type Metadata = ();
        fn receive(&mut self, out: &mut [u8]) -> Result<weave::ReceivedFrame<()>, ()> {
            out.get_mut(..self.0.len())
                .ok_or(())?
                .copy_from_slice(self.0);
            Ok(weave::ReceivedFrame {
                len: self.0.len(),
                metadata: (),
            })
        }
    }
    // Each test endpoint contains exactly the dedicated response to this call.
    impl weave::correlation::ReplyReceiver for Reply {}

    let mut request_buffer = [0xaa; 8];
    let mut response_buffer = [0xaa; 8];
    let mut client = client_only::Client::new(
        Sender(Vec::new()),
        &mut request_buffer,
        &mut response_buffer,
    );
    assert_eq!(
        client
            .r#type(&mut Reply(&[1, 99, 0]), Number(23))
            .unwrap()
            .value,
        99u16
    );
    client.ping(&mut Reply(&[2]), ()).unwrap();
    assert!(matches!(
        client.r#type(&mut Reply(&[1, 99, 0, 4]), Number(5)),
        Err(weave::request_reply::OperationError::Call(
            weave::request_reply::CallError::Receive(weave::transport::ReceiveError::Decode)
        ))
    ));
    let sender = client.into_inner().into_sender();
    assert_eq!(sender.0, vec![vec![1, 23, 0], vec![2], vec![1, 5, 0]]);
}

#[renamed::interface(tag = u8, dispatch)]
#[weave(crate = crate::renamed)]
mod alias {
    #[operation(code = 1, request = [u8; 4], response = (), handler = r#type)]
    pub struct Echo;
}

#[test]
fn explicit_path_instantiated_payload_types_and_raw_handler_names_work() {
    struct Service;
    impl alias::Handler for Service {
        type Error = ();
        fn r#type(&mut self, request: [u8; 4]) -> Result<(), ()> {
            assert_eq!(request, [1, 2, 3, 4]);
            Ok(())
        }
    }
    assert_eq!(alias::Echo::TAG.code(), 1);
    let request = alias::Request::decode_exact(&[1, 1, 2, 3, 4]).unwrap();
    let response = alias::dispatch(&mut Service, request).unwrap();
    let mut bytes = [0; 1];
    assert_eq!(response.encode(&mut bytes), Some(1));
    assert!(matches!(
        alias::Response::decode_exact(&bytes),
        Some(alias::DecodedResponse::Echo(()))
    ));
}

#[cfg(feature = "alloc")]
mod variable {
    use weave::{Codec, EncodedSize};

    #[derive(weave::Frame)]
    pub struct Blob {
        #[frame(bytes(prefix = u16, max = 16))]
        pub bytes: Vec<u8>,
    }

    #[weave::interface(tag = u8)]
    pub mod api {
        use super::Blob;
        #[operation(code = 1, request = Blob, response = Blob)]
        pub struct Put;
        #[operation(code = 2, request = (), response = u32)]
        pub struct Count;
    }

    #[test]
    fn variable_payloads_obey_branch_bounds_and_exact_frame_boundaries() {
        assert_eq!(api::Request::SIZE, EncodedSize::Bounded(19));
        assert_eq!(api::Response::SIZE, EncodedSize::Bounded(19));
        assert_eq!(api::Put::TAG, api::Operation::Put);
        assert_eq!(api::Count::TAG, api::Operation::Count);
        for length in [0, 1, 16] {
            let request = api::Request::Put(Blob {
                bytes: vec![5; length],
            });
            let mut out = [0; 64];
            let used = request.encode(&mut out).unwrap();
            assert_eq!(used, length + 3);
            let decoded = api::Request::decode_exact(&out[..used]).unwrap();
            assert!(
                matches!(decoded, api::DecodedRequest::Put(blob) if blob.bytes == vec![5; length])
            );
            assert!(api::Request::decode_exact(&out).is_none());
            for end in 0..used {
                assert!(api::Request::decode(&out[..end]).is_none());
            }
        }
        let request = api::Request::Put(Blob { bytes: vec![0; 17] });
        assert!(request.encode(&mut [0; 64]).is_none());
        assert!(api::Request::decode(&[1, 17, 0]).is_none());
    }
}
