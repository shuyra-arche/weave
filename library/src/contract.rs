use crate::Codec;

/// A request/response type association, independent of transport and routing.
pub trait Contract {
    type Request: Codec;
    type Response: Codec;
}

/// Compose a typed operation with its interface's tagged envelopes.
///
/// Implementations wrap one request variant and accept only its matching response
/// variant. Transport-level request correlation remains a separate mechanism.
pub trait Operation: Contract {
    type Interface: Contract;

    fn wrap_request(request: Self::Request) -> <Self::Interface as Contract>::Request;

    fn unwrap_response(
        response: <<Self::Interface as Contract>::Response as Codec>::Decoded,
    ) -> Option<<Self::Response as Codec>::Decoded>;
}
