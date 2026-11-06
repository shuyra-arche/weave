//! Composable encoding mechanisms for `no_std` applications.
//!
//! Integers use little-endian encoding. Callers provide storage. Enable `alloc`
//! for length-prefixed owned bytes, text, and sequences.
#![no_std]
#![doc = include_str!("../../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;
extern crate self as weave;

pub mod client;
pub mod code;
pub mod codec;
pub mod contract;
pub mod correlation;
pub mod request_reply;
pub mod server;
pub mod size;
pub mod transport;

pub use client::Client;
pub use code::{Code, Coded};
pub use codec::Codec;
pub use contract::{Contract, Operation};
pub use server::Server;
pub use size::EncodedSize;
pub use transport::{FrameReceiver, FrameSender, Received, ReceivedFrame};

#[cfg(feature = "derive")]
pub use weave_macros::{Code, Contract, Frame, interface};
