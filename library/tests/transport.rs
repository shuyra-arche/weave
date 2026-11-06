use std::cell::Cell;

use weave::correlation::{CheckedReceiver, ValidationError};
use weave::request_reply::{self, CallError};
use weave::transport::{self, ReceiveError, SendError};
use weave::{Codec, Contract, EncodedSize, FrameReceiver, FrameSender, ReceivedFrame};

#[derive(Debug, PartialEq, Eq)]
enum IoError {
    Disconnected,
    Capacity,
    Empty,
}

#[derive(Default)]
struct Sender {
    frames: Vec<Vec<u8>>,
    attempts: usize,
    disconnected: bool,
}

impl FrameSender for Sender {
    type Error = IoError;

    fn send(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        self.attempts += 1;
        if self.disconnected {
            return Err(IoError::Disconnected);
        }
        self.frames.push(bytes.to_vec());
        Ok(())
    }
}

struct Receiver {
    frame: Option<(Vec<u8>, u32)>,
    attempts: usize,
}

impl Receiver {
    fn new(bytes: &[u8], id: u32) -> Self {
        Self {
            frame: Some((bytes.to_vec(), id)),
            attempts: 0,
        }
    }
}

impl FrameReceiver for Receiver {
    type Error = IoError;
    type Metadata = u32;

    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<u32>, IoError> {
        self.attempts += 1;
        // This test backend consumes frames even when the buffer is too small.
        let (bytes, metadata) = self.frame.take().ok_or(IoError::Empty)?;
        out.get_mut(..bytes.len())
            .ok_or(IoError::Capacity)?
            .copy_from_slice(&bytes);
        Ok(ReceivedFrame {
            len: bytes.len(),
            metadata,
        })
    }
}

struct Query;
impl Contract for Query {
    type Request = u16;
    type Response = bool;
}

fn associated(metadata: &u32, _: &[u8]) -> Result<(), &'static str> {
    if *metadata == 42 {
        Ok(())
    } else {
        Err("wrong request")
    }
}

#[test]
fn typed_send_uses_only_the_encoded_range() {
    let mut sender = Sender::default();
    let mut buffer = [0xaa; 64];
    transport::send(&mut sender, &0x1234u16, &mut buffer).unwrap();
    assert_eq!(sender.frames, vec![vec![0x34, 0x12]]);
    assert_eq!(&buffer[2..], &[0xaa; 62]);
}

#[test]
fn typed_receive_preserves_metadata_and_enforces_complete_consumption() {
    let mut receiver = Receiver::new(&[0x34, 0x12], 42);
    let received = transport::receive::<u16, _>(&mut receiver, &mut [0; 64]).unwrap();
    assert_eq!(received.value, 0x1234);
    assert_eq!(received.metadata, 42);
    let mut trailing = Receiver::new(&[1, 0, 7], 42);
    assert_eq!(
        transport::receive::<u16, _>(&mut trailing, &mut [0; 64]),
        Err(ReceiveError::Decode)
    );
    let mut truncated = Receiver::new(&[1], 42);
    assert_eq!(
        transport::receive::<u16, _>(&mut truncated, &mut [0; 64]),
        Err(ReceiveError::Decode)
    );
}

#[test]
fn call_composes_checked_association_and_returns_metadata() {
    let mut sender = Sender::default();
    let mut receiver = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    let reply = request_reply::call::<Query, _, _>(
        &mut sender,
        &mut receiver,
        &99,
        &mut [0; 16],
        &mut [0; 16],
    )
    .unwrap();
    assert!(reply.value);
    assert_eq!(reply.metadata, 42);
    assert_eq!(sender.frames, vec![vec![99, 0]]);
    assert_eq!(receiver.into_inner().attempts, 1);
}

#[test]
fn encode_failure_has_no_transport_effects() {
    let mut sender = Sender::default();
    let mut receiver = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    let result = request_reply::call::<Query, _, _>(
        &mut sender,
        &mut receiver,
        &99,
        &mut [0; 1],
        &mut [0; 16],
    );
    assert_eq!(result, Err(CallError::Send(SendError::Encode)));
    assert_eq!(sender.attempts, 0);
    assert_eq!(receiver.into_inner().attempts, 0);
}

#[test]
fn send_failure_does_not_receive_or_retry() {
    let mut sender = Sender {
        disconnected: true,
        ..Sender::default()
    };
    let mut receiver = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    let result = request_reply::call::<Query, _, _>(
        &mut sender,
        &mut receiver,
        &99,
        &mut [0; 16],
        &mut [0; 16],
    );
    assert_eq!(
        result,
        Err(CallError::Send(SendError::Transport(IoError::Disconnected)))
    );
    assert_eq!(sender.attempts, 1);
    assert_eq!(receiver.into_inner().attempts, 0);
}

#[test]
fn capacity_failure_is_preserved_without_an_implicit_second_receive() {
    let mut sender = Sender::default();
    let mut receiver = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    let result =
        request_reply::call::<Query, _, _>(&mut sender, &mut receiver, &99, &mut [0; 16], &mut []);
    assert_eq!(
        result,
        Err(CallError::Receive(ReceiveError::Transport(
            ValidationError::Transport(IoError::Capacity)
        )))
    );
    assert_eq!(sender.attempts, 1);
    assert_eq!(receiver.into_inner().attempts, 1);
}

#[test]
fn mismatched_replies_are_rejected_before_payload_decoding() {
    struct MustNotDecode;
    impl Codec for MustNotDecode {
        type Decoded = ();
        const SIZE: EncodedSize = EncodedSize::Fixed(0);
        fn encode(&self, _: &mut [u8]) -> Option<usize> {
            Some(0)
        }
        fn decode(_: &[u8]) -> Option<((), usize)> {
            panic!("unassociated reply reached payload decoder")
        }
    }
    struct Call;
    impl Contract for Call {
        type Request = ();
        type Response = MustNotDecode;
    }
    let mut sender = Sender::default();
    let mut receiver = CheckedReceiver::new(Receiver::new(&[], 43), associated);
    let result =
        request_reply::call::<Call, _, _>(&mut sender, &mut receiver, &(), &mut [], &mut []);
    assert_eq!(
        result,
        Err(CallError::Receive(ReceiveError::Transport(
            ValidationError::Rejected("wrong request")
        )))
    );
    assert_eq!(receiver.into_inner().attempts, 1);
}

struct InvalidLength;
impl FrameReceiver for InvalidLength {
    type Error = ();
    type Metadata = ();
    fn receive(&mut self, _: &mut [u8]) -> Result<ReceivedFrame<()>, ()> {
        Ok(ReceivedFrame {
            len: usize::MAX,
            metadata: (),
        })
    }
}

#[test]
fn invalid_backend_lengths_fail_before_validation_or_decoding() {
    let validations = Cell::new(0);
    let mut receiver = CheckedReceiver::new(InvalidLength, |_: &(), _: &[u8]| {
        validations.set(validations.get() + 1);
        Ok::<(), ()>(())
    });
    assert_eq!(
        receiver.receive(&mut [0; 2]),
        Err(ValidationError::InvalidLength {
            reported: usize::MAX,
            capacity: 2
        })
    );
    assert_eq!(validations.get(), 0);
    assert_eq!(
        transport::receive::<u16, _>(&mut InvalidLength, &mut [0; 2]),
        Err(ReceiveError::InvalidLength {
            reported: usize::MAX,
            capacity: 2
        })
    );
}

#[test]
fn codec_length_guarantees_are_checked_even_for_custom_implementations() {
    struct Invalid;
    impl Codec for Invalid {
        type Decoded = ();
        const SIZE: EncodedSize = EncodedSize::Fixed(1);
        fn encode(&self, _: &mut [u8]) -> Option<usize> {
            Some(99)
        }
        fn decode(_: &[u8]) -> Option<((), usize)> {
            Some(((), 0))
        }
        fn decode_exact(_: &[u8]) -> Option<()> {
            Some(())
        }
    }
    let mut sender = Sender::default();
    assert_eq!(
        transport::send(&mut sender, &Invalid, &mut [0; 16]),
        Err(SendError::Encode)
    );
    assert_eq!(sender.attempts, 0);
    assert_eq!(
        transport::receive::<Invalid, _>(&mut Receiver::new(&[1], 42), &mut [0; 16]),
        Err(ReceiveError::Decode)
    );
}

#[test]
fn association_owner_controls_cleanup_on_success_failure_and_cancellation() {
    struct Lease<'a> {
        receiver: Receiver,
        releases: &'a Cell<usize>,
    }
    impl Drop for Lease<'_> {
        fn drop(&mut self) {
            self.releases.set(self.releases.get() + 1);
        }
    }
    impl FrameReceiver for Lease<'_> {
        type Error = IoError;
        type Metadata = u32;
        fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<u32>, IoError> {
            self.receiver.receive(out)
        }
    }
    let releases = Cell::new(0);
    for id in [42, 43] {
        let mut receiver = CheckedReceiver::new(
            Lease {
                receiver: Receiver::new(&[1], id),
                releases: &releases,
            },
            associated,
        );
        let result = request_reply::call::<Query, _, _>(
            &mut Sender::default(),
            &mut receiver,
            &0,
            &mut [0; 2],
            &mut [0; 1],
        );
        assert_eq!(result.is_ok(), id == 42);
    }
    assert_eq!(releases.get(), 2);
    let prepared = CheckedReceiver::new(
        Lease {
            receiver: Receiver::new(&[1], 42),
            releases: &releases,
        },
        associated,
    );
    drop(prepared);
    assert_eq!(releases.get(), 3);
}

#[test]
fn borrowed_trait_object_endpoints_compose() {
    let mut sender = Sender::default();
    let mut receiver = Receiver::new(&[1], 42);
    let mut checked = CheckedReceiver::new(&mut receiver, associated);
    let sink: &mut dyn FrameSender<Error = IoError> = &mut sender;
    let reply =
        request_reply::call::<Query, _, _>(sink, &mut checked, &4, &mut [0; 2], &mut [0; 1])
            .unwrap();
    assert!(reply.value);
    assert_eq!(receiver.attempts, 1);
}

#[test]
fn client_supports_borrowed_trait_objects_and_releases_buffers() {
    let mut sender = Sender::default();
    let mut receiver = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    let mut request_buffer = [0xaa; 8];
    let mut response_buffer = [0xaa; 8];
    let endpoint: &mut dyn FrameSender<Error = IoError> = &mut sender;
    let reply: &mut dyn weave::correlation::ReplyReceiver<
        Error = ValidationError<IoError, &'static str>,
        Metadata = u32,
    > = &mut receiver;
    let mut client = weave::Client::new(endpoint, &mut request_buffer, &mut response_buffer);
    let received = client.call::<Query, _>(reply, &99).unwrap();
    assert!(received.value);
    assert_eq!(received.metadata, 42);
    let _endpoint = client.into_sender();
    assert_eq!(&request_buffer[..2], &[99, 0]);
    assert_eq!(&response_buffer[..1], &[1]);
    assert_eq!(sender.frames, vec![vec![99, 0]]);
}

#[test]
fn client_preserves_failures_and_allows_explicit_sender_recovery() {
    let mut request_buffer = [0; 2];
    let mut response_buffer = [0; 1];
    let sender = Sender {
        disconnected: true,
        ..Sender::default()
    };
    let mut client = weave::Client::new(sender, &mut request_buffer, &mut response_buffer);
    let mut reply = CheckedReceiver::new(Receiver::new(&[1], 42), associated);
    assert_eq!(
        client.call::<Query, _>(&mut reply, &1),
        Err(CallError::Send(SendError::Transport(IoError::Disconnected)))
    );
    assert_eq!(client.sender().attempts, 1);
    assert_eq!(reply.into_inner().attempts, 0);

    // Recovery and reply association for the next call are explicit.
    client.sender_mut().disconnected = false;
    let mut reply = CheckedReceiver::new(Receiver::new(&[0], 42), associated);
    assert!(!client.call::<Query, _>(&mut reply, &2).unwrap().value);
    let sender = client.into_sender();
    assert_eq!(sender.attempts, 2);
    assert_eq!(sender.frames, vec![vec![2, 0]]);
}
