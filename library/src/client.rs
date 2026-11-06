//! Reusable synchronous calls with caller-supplied storage.

use crate::correlation::ReplyReceiver;
use crate::request_reply::{self, CallResult, OperationResult};
use crate::{Contract, FrameSender, Operation};

/// A sender and two reusable encoding/receiving buffers.
///
/// `S` can own an endpoint or borrow one through `&mut S`. Each call takes its
/// own prepared reply association. The client never retains that association or
/// retries a failed call. Before reuse after failure, the endpoint owner must
/// perform any recovery required by the backend.
pub struct Client<'a, S> {
    sender: S,
    request_buffer: &'a mut [u8],
    response_buffer: &'a mut [u8],
}

impl<'a, S> Client<'a, S> {
    pub fn new(sender: S, request_buffer: &'a mut [u8], response_buffer: &'a mut [u8]) -> Self {
        Self {
            sender,
            request_buffer,
            response_buffer,
        }
    }

    pub fn sender(&self) -> &S {
        &self.sender
    }

    pub fn sender_mut(&mut self) -> &mut S {
        &mut self.sender
    }

    /// Release the buffer borrows and recover the owned or borrowed sender.
    pub fn into_sender(self) -> S {
        self.sender
    }
}

impl<S: FrameSender> Client<'_, S> {
    /// Send the contract's request representation and receive its exact response.
    pub fn call<C: Contract, R: ReplyReceiver + ?Sized>(
        &mut self,
        receiver: &mut R,
        request: &C::Request,
    ) -> CallResult<C, S, R> {
        request_reply::call::<C, S, R>(
            &mut self.sender,
            receiver,
            request,
            self.request_buffer,
            self.response_buffer,
        )
    }

    /// Wrap an operation payload and extract its matching decoded response.
    pub fn call_operation<O: Operation, R: ReplyReceiver + ?Sized>(
        &mut self,
        receiver: &mut R,
        request: O::Request,
    ) -> OperationResult<O, S, R> {
        request_reply::call_operation::<O, S, R>(
            &mut self.sender,
            receiver,
            request,
            self.request_buffer,
            self.response_buffer,
        )
    }
}
