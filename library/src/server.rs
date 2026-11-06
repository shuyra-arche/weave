//! One synchronous receive, dispatch, and reply, with explicit reply routing.

use crate::transport::{self, ReceiveError, SendError};
use crate::{Codec, Contract, FrameReceiver, FrameSender};

/// The stage at which serving one request failed.
/// Dispatch and send errors may follow application side effects.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError<R, P, H, S> {
    Receive(ReceiveError<R>),
    Route(P),
    Dispatch(H),
    Send(SendError<S>),
}

pub type ServeResult<R, S, P, H> =
    Result<(), ServeError<<R as FrameReceiver>::Error, P, H, <S as FrameSender>::Error>>;

/// Receive and decode one complete request, prepare its reply route, dispatch
/// once, and send one encoded response.
///
/// `route` must validate/select a reply destination for this request. It receives
/// the decoded request by reference and owns the transport metadata, allowing it
/// to move a non-Clone reply capability into the returned sender. The sender may
/// instead borrow an external endpoint. It cannot borrow the temporary request.
///
/// Routing runs before dispatch; a routing error prevents handler side effects.
/// The prepared sender is dropped on every subsequent return path; a borrowed
/// endpoint remains owned by its caller. Backend
/// adapters own cleanup, source validation, and any recovery after failures.
/// Handler errors are local failures; protocol errors belong in response values.
/// This function neither retries nor sends an automatic error response. A reply
/// encoding or send failure may occur after the handler has executed.
pub fn serve_once<C, R, S, P, H>(
    receiver: &mut R,
    route: impl FnOnce(&<C::Request as Codec>::Decoded, R::Metadata) -> Result<S, P>,
    dispatch: impl FnOnce(<C::Request as Codec>::Decoded) -> Result<C::Response, H>,
    request_buffer: &mut [u8],
    response_buffer: &mut [u8],
) -> ServeResult<R, S, P, H>
where
    C: Contract,
    R: FrameReceiver + ?Sized,
    S: FrameSender,
{
    let received = transport::receive::<C::Request, R>(receiver, request_buffer)
        .map_err(ServeError::Receive)?;
    let mut sender = route(&received.value, received.metadata).map_err(ServeError::Route)?;
    let response = dispatch(received.value).map_err(ServeError::Dispatch)?;
    transport::send(&mut sender, &response, response_buffer).map_err(ServeError::Send)
}

/// A receiver and two caller-provided buffers, reused serially for serving.
///
/// Each call supplies its own routing and dispatch mechanisms. There is no
/// implicit service loop, retry, or retained per-request reply destination.
pub struct Server<'a, R> {
    receiver: R,
    request_buffer: &'a mut [u8],
    response_buffer: &'a mut [u8],
}

impl<'a, R> Server<'a, R> {
    pub fn new(receiver: R, request_buffer: &'a mut [u8], response_buffer: &'a mut [u8]) -> Self {
        Self {
            receiver,
            request_buffer,
            response_buffer,
        }
    }

    pub fn receiver(&self) -> &R {
        &self.receiver
    }

    pub fn receiver_mut(&mut self) -> &mut R {
        &mut self.receiver
    }

    /// Release the buffer borrows and recover the owned or borrowed receiver.
    pub fn into_receiver(self) -> R {
        self.receiver
    }
}

impl<R: FrameReceiver> Server<'_, R> {
    pub fn serve_once<C, S, P, H>(
        &mut self,
        route: impl FnOnce(&<C::Request as Codec>::Decoded, R::Metadata) -> Result<S, P>,
        dispatch: impl FnOnce(<C::Request as Codec>::Decoded) -> Result<C::Response, H>,
    ) -> ServeResult<R, S, P, H>
    where
        C: Contract,
        S: FrameSender,
    {
        serve_once::<C, R, S, P, H>(
            &mut self.receiver,
            route,
            dispatch,
            self.request_buffer,
            self.response_buffer,
        )
    }
}
