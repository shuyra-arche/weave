//! One synchronous request/reply composed from public codec and transport mechanisms.

use crate::correlation::ReplyReceiver;
use crate::transport::{self, FrameReceiver, FrameSender, ReceiveError, Received, SendError};
use crate::{Codec, Contract, Operation};

#[derive(Debug, PartialEq, Eq)]
pub enum CallError<S, R> {
    Send(SendError<S>),
    Receive(ReceiveError<R>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum OperationError<S, R> {
    Call(CallError<S, R>),
    WrongOperation,
}

pub type CallResult<C, S, R> = Result<
    Received<<<C as Contract>::Response as Codec>::Decoded, <R as FrameReceiver>::Metadata>,
    CallError<<S as FrameSender>::Error, <R as FrameReceiver>::Error>,
>;

pub type OperationResult<O, S, R> = Result<
    Received<<<O as Contract>::Response as Codec>::Decoded, <R as FrameReceiver>::Metadata>,
    OperationError<<S as FrameSender>::Error, <R as FrameReceiver>::Error>,
>;

/// Encode, send once, receive one associated reply, and decode it exactly.
///
/// The caller prepares the association before constructing `request`. Buffers and
/// endpoints remain caller-owned. On any failure, this function returns immediately;
/// a send/receive failure may occur after the peer has executed the request.
/// Cancellation, time budgets, cleanup, and reuse after failure belong to the
/// endpoint/association owner. This function does not retry operations.
pub fn call<C, S, R>(
    sender: &mut S,
    receiver: &mut R,
    request: &C::Request,
    request_buffer: &mut [u8],
    response_buffer: &mut [u8],
) -> CallResult<C, S, R>
where
    C: Contract,
    S: FrameSender + ?Sized,
    R: ReplyReceiver + ?Sized,
{
    transport::send(sender, request, request_buffer).map_err(CallError::Send)?;
    transport::receive::<C::Response, R>(receiver, response_buffer).map_err(CallError::Receive)
}

/// Call one typed operation through its tagged interface envelope.
/// The returned value is the operation's decoded response, with source metadata.
pub fn call_operation<O, S, R>(
    sender: &mut S,
    receiver: &mut R,
    request: O::Request,
    request_buffer: &mut [u8],
    response_buffer: &mut [u8],
) -> OperationResult<O, S, R>
where
    O: Operation,
    S: FrameSender + ?Sized,
    R: ReplyReceiver + ?Sized,
{
    let frame = O::wrap_request(request);
    let received =
        call::<O::Interface, S, R>(sender, receiver, &frame, request_buffer, response_buffer)
            .map_err(OperationError::Call)?;
    let value = O::unwrap_response(received.value).ok_or(OperationError::WrongOperation)?;
    Ok(Received {
        value,
        metadata: received.metadata,
    })
}
