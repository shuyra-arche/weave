//! Explicit reply association over complete-frame receivers.
//!
//! Prepare the reply route before constructing the request. Route resources and
//! cancellation are owned by the adapter, which can use ordinary RAII cleanup.

use crate::transport::{FrameReceiver, ReceivedFrame};

/// A receiver bound to the current request's reply association.
///
/// Implementors must return success only for an associated reply. An adapter may
/// use an exclusive reply endpoint, a correlation identifier, and/or authenticated
/// source metadata. Rebinding for another request must preserve that guarantee.
/// Implementing this trait is an explicit declaration of the association contract;
/// there is no blanket implementation for arbitrary frame receivers.
pub trait ReplyReceiver: FrameReceiver {}

impl<T: ReplyReceiver + ?Sized> ReplyReceiver for &mut T {}

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError<R, V> {
    Transport(R),
    InvalidLength { reported: usize, capacity: usize },
    Rejected(V),
}

/// Validate transport metadata and raw reply bytes before typed decoding.
///
/// The validator must establish the complete association for the current request.
/// A mismatch returns immediately; this adapter does not skip unrelated frames,
/// retry reception, or impose a correlation field on the wire representation.
pub struct CheckedReceiver<R, V> {
    receiver: R,
    validate: V,
}

impl<R, V> CheckedReceiver<R, V> {
    pub fn new(receiver: R, validate: V) -> Self {
        Self { receiver, validate }
    }

    /// Recover the underlying receiver for application-controlled recovery.
    pub fn into_inner(self) -> R {
        self.receiver
    }
}

impl<R, V, E> FrameReceiver for CheckedReceiver<R, V>
where
    R: FrameReceiver,
    V: FnMut(&R::Metadata, &[u8]) -> Result<(), E>,
{
    type Error = ValidationError<R::Error, E>;
    type Metadata = R::Metadata;

    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<Self::Metadata>, Self::Error> {
        let received = self
            .receiver
            .receive(out)
            .map_err(ValidationError::Transport)?;
        let bytes = out
            .get(..received.len)
            .ok_or(ValidationError::InvalidLength {
                reported: received.len,
                capacity: out.len(),
            })?;
        (self.validate)(&received.metadata, bytes).map_err(ValidationError::Rejected)?;
        Ok(received)
    }
}

impl<R, V, E> ReplyReceiver for CheckedReceiver<R, V>
where
    R: FrameReceiver,
    V: FnMut(&R::Metadata, &[u8]) -> Result<(), E>,
{
}
