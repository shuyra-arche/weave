//! Complete-frame I/O and typed codec adapters.
//!
//! Backends define how frames are delimited and whether a failed operation has
//! consumed input or produced output. An error never implies that retrying is safe.

use crate::{Codec, codec};

/// A successful send submits exactly one complete frame.
/// Success does not imply that the peer has executed a request.
pub trait FrameSender {
    type Error;

    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// The valid input range and the transport facts accompanying a received frame.
#[derive(Debug, PartialEq, Eq)]
pub struct ReceivedFrame<M> {
    pub len: usize,
    pub metadata: M,
}

/// A successful receive writes one complete frame into `out[..len]`.
///
/// A truncated frame must be reported as an error. Backends must document whether
/// capacity errors retain or discard the frame and whether the endpoint can be reused.
pub trait FrameReceiver {
    type Error;
    type Metadata;

    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<Self::Metadata>, Self::Error>;
}

impl<T: FrameSender + ?Sized> FrameSender for &mut T {
    type Error = T::Error;

    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        T::send(self, bytes)
    }
}

impl<T: FrameReceiver + ?Sized> FrameReceiver for &mut T {
    type Error = T::Error;
    type Metadata = T::Metadata;

    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<Self::Metadata>, Self::Error> {
        T::receive(self, out)
    }
}

/// Decoded data together with its transport metadata.
#[derive(Debug, PartialEq, Eq)]
pub struct Received<T, M> {
    pub value: T,
    pub metadata: M,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError<E> {
    Encode,
    Transport(E),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveError<E> {
    Transport(E),
    InvalidLength { reported: usize, capacity: usize },
    Decode,
}

pub type ReceiveResult<T, R> = Result<
    Received<<T as Codec>::Decoded, <R as FrameReceiver>::Metadata>,
    ReceiveError<<R as FrameReceiver>::Error>,
>;

/// Encode and submit only the bytes actually written.
/// An encoding failure does not invoke the sender.
pub fn send<T: Codec, S: FrameSender + ?Sized>(
    sender: &mut S,
    value: &T,
    buffer: &mut [u8],
) -> Result<(), SendError<S::Error>> {
    let used = codec::encode(value, buffer).ok_or(SendError::Encode)?;
    sender.send(&buffer[..used]).map_err(SendError::Transport)
}

/// Receive one complete frame and decode its exact valid range.
pub fn receive<T: Codec, R: FrameReceiver + ?Sized>(
    receiver: &mut R,
    buffer: &mut [u8],
) -> ReceiveResult<T, R> {
    let ReceivedFrame { len, metadata } =
        receiver.receive(buffer).map_err(ReceiveError::Transport)?;
    let bytes = buffer.get(..len).ok_or(ReceiveError::InvalidLength {
        reported: len,
        capacity: buffer.len(),
    })?;
    // Use checked composition directly so an overridden decode_exact cannot
    // weaken the full-frame boundary imposed by this adapter.
    let (value, used) = codec::decode::<T>(bytes).ok_or(ReceiveError::Decode)?;
    if used != bytes.len() {
        return Err(ReceiveError::Decode);
    }
    Ok(Received { value, metadata })
}
