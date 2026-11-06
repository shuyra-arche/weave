# Weave

Weave composes value codecs, size guarantees, explicit numeric mappings,
complete-frame transport, and request/reply execution.
The workspace contains the `weave` library and the `weave-macros` proc-macro
crate. The library re-exports its derives through the default `derive` feature.

The core uses `no_std`. Enable `alloc` for owned variable-length fields.
Callers supply buffers. Fixed-width integers use little-endian encoding.

## Core contract

`Codec::encode` returns the number of bytes written. `Codec::decode` returns
the decoded value and the number of bytes consumed from the input prefix.
`decode_exact` rejects trailing bytes. `Decoded` may differ from the encoding
type. A failed encode may leave partial writes, which the caller must discard.

`EncodedSize::Fixed(n)` promises exactly `n` bytes on success. `Bounded(n)`
promises at most `n`. `Unbounded` supplies no finite upper bound. Buffer capacity
is the actual slice length and is independent of these guarantees. Checked
composition helpers restrict slices and verify reported consumption.

```rust
use weave::{Codec, EncodedSize};

let mut bytes = [0u8; 8];
assert_eq!(0x1234u16.encode(&mut bytes), Some(2));
assert_eq!(&bytes[..2], &[0x34, 0x12]);
assert_eq!(u16::decode_exact(&bytes[..2]), Some(0x1234));
assert_eq!(u16::decode_exact(&bytes), None);
assert_eq!(u16::SIZE, EncodedSize::Fixed(2));
```

## Frame

`Frame` derives `Codec<Decoded = Self>` for named-field structs. Plain fields
must implement `Codec<Decoded = FieldType>`. Generic parameters, const generics,
and where clauses are preserved.

```rust
# #[cfg(feature = "derive")] {
use weave::{Codec, EncodedSize};

#[derive(Debug, PartialEq, weave::Frame)]
struct Header {
    version: u8,
    sequence: u32,
}

assert_eq!(Header::SIZE, EncodedSize::Fixed(5));
let value = Header { version: 1, sequence: 7 };
let mut out = [0u8; 5];
assert_eq!(value.encode(&mut out), Some(5));
assert_eq!(Header::decode_exact(&out), Some(value));
# }
```

Variable-length field rules are explicit.

| Rule | Field | Prefix unit |
| --- | --- | --- |
| `bytes(prefix = u16, max = N)` | `Vec<u8>` | bytes |
| `text(prefix = u16, max = N)` | `String` | UTF-8 bytes |
| `sequence(prefix = u16, max = N)` | `Vec<T>` | elements |
| `sequence(count = count, fill = value)` | `[T; N]` | earlier count field |

Prefix and count types are `u8`, `u16`, `u32`, `u64`, or `u128`. The prefix's
range must cover the declared maximum. Conversions into platform lengths are
checked. A count field must appear earlier and its value must fit the array.
Unused array elements are initialized with `fill` after decoding active elements.
Type aliases for `Vec`, `String`, and count types are outside this initial derive
grammar; their codecs can be implemented using the same public helpers.

```rust
# #[cfg(all(feature = "derive", feature = "alloc"))] {
use weave::{Codec, EncodedSize};

#[derive(Debug, PartialEq, weave::Frame)]
struct Packet {
    version: u8,
    #[frame(bytes(prefix = u16, max = 4096))]
    payload: Vec<u8>,
    checksum: u32,
}

assert_eq!(Packet::SIZE, EncodedSize::Bounded(4103));
let packet = Packet { version: 1, payload: vec![10, 20], checksum: 42 };
let mut out = [0u8; 9];
assert_eq!(packet.encode(&mut out), Some(9));
assert_eq!(Packet::decode_exact(&out), Some(packet));
# }
```

Struct-level `#[frame(max = N)]` imposes a byte limit on the entire frame,
including prefixes. The implementation restricts readable and writable slices
before visiting fields. Fixed sizes remain fixed when they fit the limit.
Size arithmetic overflow and impossible fixed limits are declaration errors.

The public functions in `codec::bounded` implement these mechanisms directly.
Borrowed byte/text decoding and counted arrays work without allocation. Owned
decoders use checked allocation, returning `None` if reservation fails.

## Code

`Code` derives explicit enum-to-integer mappings. Every variant must be fieldless
and have a discriminant. Use a fixed-width integer `repr`. Rust validates ranges
and duplicate discriminants. Sparse and signed codes are supported. Unknown
values return `None`. Success, retry, and fallback policies belong to callers.

```rust
# #[cfg(feature = "derive")] {
use weave::{Code, Codec, Coded};

#[derive(Debug, PartialEq, weave::Code)]
#[repr(u8)]
enum Operation {
    Read = 1,
    Write = 7,
}

assert_eq!(Operation::Write.code(), 7);
assert_eq!(Operation::from_code(2), None);
assert_eq!(Coded::<Operation>::decode_exact(&[1]), Some(Coded(Operation::Read)));

#[derive(weave::Frame)]
struct Request {
    #[frame(code)]
    operation: Operation,
    offset: u64,
}
let request = Request { operation: Operation::Read, offset: 12 };
assert_eq!(request.encode(&mut [0u8; 9]), Some(9));
# }
```

`Coded<T>` combines `Code` with the integer representation's `Codec`.
The `frame(code)` attribute provides the same composition while keeping the
original enum as the struct field type.

## Contract and paths

`Contract` associates request and response codecs. Its derive accepts
`#[contract(request = RequestType, response = ResponseType)]` on a struct.
Routing, transport, and execution are owned by the consuming component.

All derives accept `#[weave(crate = path)]` when the library is renamed or
re-exported.

## Interface

`interface` composes `Code`, `Contract`, and codecs from one explicit operation
table. Each operation is a nongeneric unit struct with request and response
codec types. It gets `Contract` and `weave::Operation` implementations and an
interface-local `Operation`-typed `TAG`.

```rust
# #[cfg(feature = "derive")] {
use weave::{Codec, EncodedSize};

#[weave::interface(tag = u16, dispatch)]
pub mod storage {
    #[operation(code = 1, request = u32, response = u16, handler = read)]
    pub struct Read;

    #[operation(code = 7, request = u16, response = u8, handler = write)]
    pub struct Write;
}

struct Service;
impl storage::Handler for Service {
    type Error = &'static str;
    fn read(&mut self, request: u32) -> Result<u16, Self::Error> {
        u16::try_from(request).map_err(|_| "overflow")
    }
    fn write(&mut self, request: u16) -> Result<u8, Self::Error> {
        u8::try_from(request).map_err(|_| "overflow")
    }
}

assert_eq!(storage::Request::SIZE, EncodedSize::Bounded(6));
let request = storage::Request::Read(42);
let mut bytes = [0; 6];
let used = request.encode(&mut bytes).unwrap();
assert_eq!(&bytes[..used], &[1, 0, 42, 0, 0, 0]);
let decoded = storage::Request::decode_exact(&bytes[..used]).unwrap();
let response = storage::dispatch(&mut Service, decoded).unwrap();
assert!(matches!(response, storage::Response::Read(42)));
# }
```

The generated public surface includes `Interface`, `Operation`, `Request`,
`DecodedRequest`, `Response`, and `DecodedResponse`. `Interface` implements
`Contract` for the tagged request and response envelopes. Encoding uses
`[tag][payload]` in both
directions. Unknown tags fail decoding. `decode` consumes a prefix;
`decode_exact` also rejects trailing bytes. Each decoded variant contains the
payload codec's `Decoded` type, which can differ from its sending type.

`EncodedSize::choice` merges branch sizes. Equal fixed branches remain fixed,
other finite branches use their maximum, and an unbounded branch makes the
choice unbounded. The tag size is added separately. Empty choices are errors.

With `dispatch`, every operation specifies a unique `handler` identifier. The
generated `Handler` receives decoded requests and returns response encoding
types. `dispatch` propagates its local error. Protocol errors can be represented
inside response payloads. `client` independently enables named client methods,
with an explicit `call` identifier on each operation. `server` enables the server
wrapper together with `Handler` and `dispatch`. Omit `client`, `server`, and
`dispatch` and their operation options to generate the tagged codecs and contracts
alone. The operation tag identifies the operation
kind; request correlation belongs to the execution component.

Tag types are `u8`, `u16`, `u32`, `u64`, or `u128`. Codes are explicit integer
literals, including zero and sparse values. They must be unique across the
declaration, including conditional operations. `cfg` and nested `cfg_attr`
availability are propagated to every generated surface. At least one operation
must remain enabled. Generated type names, plus `Handler` and `dispatch`, are
reserved within the interface module, as are `Client` and `Server`. Other module items are preserved.
Use `#[weave(crate = path)]` on the module for a renamed library.

Each operation marker's `Contract` describes its payloads. Its
`weave::Operation` implementation wraps a request into the interface envelope
and extracts the matching decoded response. A different response branch returns
`None`. These methods are public composition points for handwritten clients too.
`storage::Operation` is the numeric tag enum; `weave::Operation` is the adapter trait.

Duplicate operation codes are rejected.

```compile_fail
#[weave::interface(tag = u8)]
mod api {
    #[operation(code = 1, request = (), response = ())] pub struct A;
    #[operation(code = 1, request = (), response = ())] pub struct B;
}
```

Every operation must name codecs, and dispatch requires handler names.

```compile_fail
#[weave::interface(tag = u8)]
mod api {
    #[operation(code = 1, request = std::path::PathBuf, response = ())]
    pub struct A;
}
```

```compile_fail
#[weave::interface(tag = u8, dispatch)]
mod api {
    #[operation(code = 1, request = (), response = ())] pub struct A;
}
```

An interface with all operations disabled is rejected.

```compile_fail
#[weave::interface(tag = u8)]
mod api {
    #[cfg(any())]
    #[operation(code = 1, request = (), response = ())] pub struct A;
}
```

## Transport and request/reply

`FrameSender::send` submits one complete byte frame. `FrameReceiver::receive`
writes one complete frame and returns `ReceivedFrame { len, metadata }`.
Both support borrowed endpoints and trait objects. A stream backend supplies
its own framing before implementing these traits. Truncation is an error;
the backend documents whether a capacity failure consumes the incoming frame.

`transport::send` encodes into caller-supplied storage and sends only the bytes
written. `transport::receive` validates the reported length and exactly decodes
that range, returning `Received { value, metadata }`. The declared codec bound,
the buffer capacity, and the actual received length remain separate quantities.

`request_reply::call::<C, _, _>` composes one send with one exact receive using
a `Contract`. `call_operation::<O, _, _>` additionally wraps an operation payload
and checks the response's operation branch. Use `api::Interface` with `call`
for tagged envelopes, or `api::Read` with `call_operation` for a typed payload.
Calling `call::<api::Read, _, _>` directly would send the payload alone.

```rust
# #[cfg(feature = "derive")] {
use weave::{FrameSender, correlation::ReplyReceiver, request_reply};

#[weave::interface(tag = u16)]
mod storage {
    #[operation(code = 1, request = u32, response = u16)]
    pub struct Read;
}

fn read<S: FrameSender, R: ReplyReceiver>(
    sender: &mut S,
    reply: &mut R,
    address: u32,
) -> request_reply::OperationResult<storage::Read, S, R> {
    request_reply::call_operation::<storage::Read, _, _>(
        sender, reply, address, &mut [0; 6], &mut [0; 4],
    )
}
# }
```

Reply association is explicit. `ReplyReceiver` declares that successful receives
belong to the current request. A dedicated reply endpoint can implement this
contract directly. `correlation::CheckedReceiver` combines an ordinary receiver
with a validator over transport metadata and raw bytes, before typed decoding.
The caller supplies the complete association rule. An operation tag alone does
not distinguish concurrent calls or stale replies to the same operation.

```rust
use weave::{FrameReceiver, correlation::CheckedReceiver};

fn associated<R: FrameReceiver<Metadata = u64>>(
    receiver: R,
    expected: u64,
) -> impl weave::correlation::ReplyReceiver<Metadata = u64> {
    // This backend defines metadata as a trusted, unique per-request route ID.
    CheckedReceiver::new(receiver, move |id: &u64, _: &[u8]| {
        if *id == expected { Ok(()) } else { Err("unassociated reply") }
    })
}
```

Prepare reply routes before constructing requests that carry their identifiers.
The caller owns endpoints, buffers, route lifetimes, and cleanup, which can use
RAII. The current execution functions are synchronous. They return immediately
on failure, perform no retry, and do not skip unrelated replies. Transport or
receive failure may occur after the peer has executed the request. Recovery,
timeouts, and endpoint reuse are adapter policies. Correlation validators may
also reject a wrong source when the backend provides that metadata.

An ordinary frame receiver cannot be passed directly to the call mechanism.

```compile_fail
use weave::{Contract, FrameSender, FrameReceiver, request_reply};

fn unassociated<C: Contract, S: FrameSender, R: FrameReceiver>(
    sender: &mut S, receiver: &mut R, request: &C::Request,
) {
    let _ = request_reply::call::<C, S, R>(
        sender, receiver, request, &mut [0; 32], &mut [0; 32],
    );
}
```

The dependency direction inside the library is
`request_reply -> contract + correlation + transport -> codec`.
Macros emit implementations of the public mechanisms; the runtime library works
without the derive feature. Platform adapters provide frame I/O, reply routing,
source metadata, and resource lifetimes. Kernel ABI types and register calls
belong in a Shuyra adapter.

## Client and named calls

`weave::Client<'a, S>` owns or borrows a sender and borrows two caller-provided
buffers. Its `call::<C, _>(reply, &request)` and
`call_operation::<O, _>(reply, request)` methods reuse the public execution
functions. It adds no allocation or wire representation. Each method takes
`&mut self`, so the buffers are reused serially, and accepts a separately prepared
`ReplyReceiver` for that call. A client stores no reply association between calls.

`sender()` and `sender_mut()` expose the endpoint for inspection or explicit
recovery. `into_sender()` consumes the client, releases its buffer borrows, and
returns the owned or borrowed endpoint. `&mut dyn FrameSender` is supported as
the sender, and calls accept `&mut dyn ReplyReceiver`.

Enable named methods with `client` and supply each method name using `call`.
Client and dispatch generation are independent; `call` and `handler` names may
be the same or different. These names affect only the Rust API. The explicit
numeric operation code controls the wire representation.

```rust
# #[cfg(feature = "derive")] {
use weave::{FrameSender, correlation::ReplyReceiver, request_reply::OperationResult};

#[weave::interface(tag = u16, client, dispatch)]
mod storage {
    #[operation(code = 1, request = u32, response = u16, call = read, handler = load)]
    pub struct Read;
}

fn read<S: FrameSender, R: ReplyReceiver>(
    sender: S,
    prepared_reply: &mut R,
    address: u32,
) -> OperationResult<storage::Read, S, R> {
    let mut request_buffer = [0; 6];
    let mut response_buffer = [0; 4];
    let mut client = storage::Client::new(sender, &mut request_buffer, &mut response_buffer);
    client.read(prepared_reply, address)
}
# }
```

Generated `storage::Client<'a, S>` is a thin wrapper around `weave::Client`.
Its methods take the operation's request value and return
`Received<Response::Decoded, Metadata>` inside `OperationResult`, preserving
transport, association, decoding, and wrong-operation errors. Request values
are moved into the tagged envelope without a `Clone` requirement. Replies may
decode to a type different from the response encoding type.

`from_inner(core_client)` wraps an existing generic client; `inner_mut()` gives
access to its generic calls and sender; `into_inner()` recovers it. These paths
allow the named API to compose with handwritten contracts. The method names
`new`, `from_inner`, `into_inner`, and `inner_mut` are reserved. Call names must
be unique, including across conditional operations and raw identifier spellings.
`cfg` and nested `cfg_attr` also control the generated methods.

The macro requires `call` exactly when `client` is enabled. It performs no
automatic case conversion or implicit reuse of handler names. For a client-only
interface, write `#[weave::interface(tag = u16, client)]` and omit `handler`.

Named calls enforce the same explicit association requirement as generic calls.

```compile_fail
#[weave::interface(tag = u8, client)]
mod api {
    #[operation(code = 1, request = (), response = (), call = ping)]
    pub struct Ping;
}
fn unassociated<S: weave::FrameSender, R: weave::FrameReceiver>(
    client: &mut api::Client<'_, S>, receiver: &mut R,
) {
    let _ = client.ping(receiver, ());
}
```

Disabled operations expose no callable method.

```compile_fail
#[weave::interface(tag = u8, client)]
mod api {
    #[operation(code = 1, request = (), response = (), call = ping)]
    pub struct Ping;
    #[cfg(any())]
    #[operation(code = 2, request = (), response = (), call = hidden)]
    pub struct Hidden;
}
fn unavailable<S: weave::FrameSender, R: weave::correlation::ReplyReceiver>(
    client: &mut api::Client<'_, S>, receiver: &mut R,
) {
    let _ = client.hidden(receiver, ());
}
```

Client reuse follows the transport's recovery rules. A failed call may already
have executed on the peer; callers decide how to recover the endpoint and prepare
the next association. Generated methods perform no retry or route allocation.
The dependency is `generated Client -> weave::Client -> request_reply`; backend
resource management remains in the platform adapter.

## Server and reply routing

`server::serve_once` composes one complete receive, exact request decoding,
reply-route preparation, dispatch, response encoding, and one send, in that
order. `weave::Server<'a, R>` reuses a receiver and two caller-provided buffers
around this function. Its generic `serve_once::<C, _, _, _>(route, dispatch)`
accepts a `Contract` and two callbacks, without requiring macros.

The routing callback has the following shape.

```text
FnOnce(&Request::Decoded, Metadata) -> Result<ReplySender, RouteError>
```

`ReplySender` here is any `FrameSender` selected for this request. The callback
owns the received metadata and borrows the decoded request. This allows a
non-Clone capability in metadata to move into an owned reply sender, or allows
the callback to return a borrowed external endpoint. The returned sender cannot
borrow the temporary request. Source validation, correlation data, and destination
selection belong to this callback and the backend. A destination carried in an
untrusted payload must be checked according to the adapter's rules before use.

Routing completes before business dispatch. Its error prevents the handler from
running. The prepared sender stays alive through dispatch and sending, and is
dropped on every subsequent return path. An owned route can release its resources
through `Drop`; a borrowed sender remains owned by its caller. Metadata cleanup
on decode failure also follows its normal ownership. Transport failures follow
the receiver's documented resource and frame-consumption rules.

Enable `server` to generate an interface-specific `Server` with
`serve_once(handler, route)`. The macro supplies the dispatch callback.

```rust
# #[cfg(feature = "derive")] {
use core::convert::Infallible;
use weave::{FrameReceiver, FrameSender, server::ServeResult};

#[weave::interface(tag = u16, server)]
mod storage {
    #[operation(code = 1, request = u32, response = u16, handler = read)]
    pub struct Read;
}

struct Service;
impl storage::Handler for Service {
    type Error = &'static str;
    fn read(&mut self, address: u32) -> Result<u16, Self::Error> {
        u16::try_from(address).map_err(|_| "invalid address")
    }
}

// The caller supplies the dedicated reply endpoint for this single request.
fn serve_dedicated<R: FrameReceiver<Metadata = ()>, S: FrameSender>(
    receiver: R, reply_sender: S,
) -> ServeResult<R, S, Infallible, &'static str> {
    let mut request_buffer = [0; 6];
    let mut response_buffer = [0; 4];
    let mut server = storage::Server::new(receiver, &mut request_buffer, &mut response_buffer);
    server.serve_once(&mut Service, |_request, ()| Ok(reply_sender))
}
# }
```

For shared request endpoints, the route callback selects a reply sender separately
for each received request. The server retains no reply destination between calls.
`client` and `server` can be enabled together; `server` implies dispatch generation,
so every operation requires `handler`. `dispatch` alone continues to generate only
the handler trait and dispatch function alongside the codecs and contracts.

```compile_fail
#[weave::interface(tag = u8, server)]
mod api {
    // A server requires a dispatch handler name for every operation.
    #[operation(code = 1, request = (), response = ())]
    pub struct Ping;
}
```

`ServeError` preserves each failure stage.

| Variant | Meaning | Later work |
| --- | --- | --- |
| `Receive` | Transport, reported length, or exact decoding failed | No routing, dispatch, or reply |
| `Route` | Preparing or validating the reply route failed | No dispatch or reply |
| `Dispatch` | The handler returned a local error | No reply |
| `Send(Encode)` | The response could not be encoded into the supplied buffer | No transport send |
| `Send(Transport)` | Sending the encoded reply failed | No retry |

A handler may already have changed application state when it fails or when its
reply cannot be encoded or sent. A successful return means the reply was submitted
to the sender; it does not acknowledge delivery to the peer. Protocol error replies
must be expressed as normal response values. The server supplies no automatic
wire error mapping, retry, service loop, or time budget.

`weave::Server` exposes `receiver()`, `receiver_mut()`, and `into_receiver()` for
inspection, recovery, and releasing buffer borrows. Generated servers expose
`from_inner()`, `inner_mut()`, and `into_inner()` like generated clients. Borrowed
receiver/sender trait objects and handler trait objects are supported. Reuse is
serial and follows the backend's recovery rules after any error.

The dependency is `generated Server -> weave::Server -> transport + Contract`.
The macro connects the existing dispatch function; runtime execution uses the same
checked transport and codec composition as the client. Shuyra-specific source
metadata, capability handling, and reply routing remain adapter responsibilities.

## Declaration diagnostics

Missing discriminants are rejected.

```compile_fail
#[derive(weave::Code)]
#[repr(u8)]
enum Bad { First = 1, Second }
```

Duplicate discriminants and values outside the representation are rejected.

```compile_fail
#[derive(weave::Code)]
#[repr(u8)]
enum Bad { First = 1, Second = 1 }
```

```compile_fail
#[derive(weave::Code)]
#[repr(u8)]
enum Bad { TooLarge = 256 }
```

Count fields must precede their arrays.

```compile_fail
#[derive(weave::Frame)]
struct Bad {
    #[frame(sequence(count = count, fill = 0))]
    items: [u16; 4],
    count: u8,
}
```

Prefix and count forms cannot be mixed.

```compile_fail
#[derive(weave::Frame)]
struct Bad {
    count: u8,
    #[frame(sequence(prefix = u8, max = 4, count = count, fill = 0))]
    items: [u16; 4],
}
```

A fixed encoding must fit the declared frame maximum.

```compile_fail
#[derive(weave::Frame)]
#[frame(max = 1)]
struct Bad { value: u32 }
const SIZE: weave::EncodedSize = <Bad as weave::Codec>::SIZE;
```

A length maximum must fit the prefix representation.

```compile_fail
const SIZE: weave::EncodedSize =
    weave::codec::bounded::prefixed_size::<u8>(weave::EncodedSize::Fixed(1), 256);
```

## Development checks

Use an installed Rust toolchain with rustfmt and Clippy. CI validates Rust
1.99.0 without imposing a local toolchain override. Install the RV64 target with
`rustup target add riscv64gc-unknown-none-elf` to check `no_std` support.
Run from the repository root:

```text
cargo test --locked --workspace --all-features
cargo test --locked -p weave --no-default-features
cargo check --locked -p weave --no-default-features --features alloc
cargo check --locked -p weave --no-default-features --features derive
cargo check --locked -p weave --no-default-features --target riscv64gc-unknown-none-elf
cargo check --locked -p weave --all-features --target riscv64gc-unknown-none-elf
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

CI runs these checks on Linux and Windows. Target compilation verifies `no_std`
compatibility; it does not exercise a platform transport or Kernel integration.

## Source releases

The workspace shares one source version. Releases use tags such as `d0.0.1`
on commits included in `main`. The release workflow verifies the version and
ancestry, runs CI, and creates a GitHub Release. Packages are not published to
crates.io, and no compiled binaries are uploaded. Consumers can pin an exact
Git revision, or select a source release:

```toml
[dependencies]
weave = { git = "https://github.com/shuyra-arche/weave.git", tag = "d0.0.1" }
```
