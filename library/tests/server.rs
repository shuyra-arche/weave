use std::cell::RefCell;
use std::rc::Rc;

use weave::server::{self, ServeError};
use weave::transport::{ReceiveError, SendError};
use weave::{Contract, FrameReceiver, FrameSender, ReceivedFrame};

#[cfg(feature = "derive")]
use weave as renamed;

type Events = Rc<RefCell<Vec<&'static str>>>;

struct Query;
impl Contract for Query {
    type Request = u16;
    type Response = u16;
}

// A non-Clone per-request capability can move from metadata into the reply sender.
struct Lease(Events);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.borrow_mut().push("release");
    }
}

struct Inbox {
    bytes: Vec<u8>,
    metadata: Option<Lease>,
    events: Events,
    fail: bool,
    invalid_length: bool,
}
impl FrameReceiver for Inbox {
    type Error = &'static str;
    type Metadata = Lease;
    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<Lease>, Self::Error> {
        self.events.borrow_mut().push("receive");
        if self.fail {
            return Err("receive");
        }
        out.get_mut(..self.bytes.len())
            .ok_or("capacity")?
            .copy_from_slice(&self.bytes);
        Ok(ReceivedFrame {
            len: if self.invalid_length {
                usize::MAX
            } else {
                self.bytes.len()
            },
            metadata: self.metadata.take().ok_or("empty")?,
        })
    }
}

struct Reply {
    lease: Lease,
    fail: bool,
    sent: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl FrameSender for Reply {
    type Error = &'static str;
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.lease.0.borrow_mut().push("send");
        if self.fail {
            return Err("send");
        }
        self.sent.borrow_mut().push(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn serving_stops_at_the_failed_stage_and_releases_prepared_routes() {
    for case in [
        "receive",
        "capacity",
        "invalid",
        "truncated",
        "trailing",
        "route",
        "dispatch",
        "encode",
        "send",
        "ok",
    ] {
        let events = Events::default();
        let sent = Rc::new(RefCell::new(Vec::new()));
        let mut inbox = Inbox {
            bytes: match case {
                "truncated" => vec![1],
                "trailing" => vec![1, 0, 0],
                _ => vec![1, 0],
            },
            metadata: Some(Lease(events.clone())),
            events: events.clone(),
            fail: case == "receive",
            invalid_length: case == "invalid",
        };
        let mut input = [0xaa; 8];
        let mut output = [0xaa; 8];
        let input_len = if case == "capacity" { 1 } else { 8 };
        let output_len = if case == "encode" { 1 } else { 8 };
        let result = server::serve_once::<Query, _, _, _, _>(
            &mut inbox,
            |request, lease| {
                events.borrow_mut().push("route");
                assert_eq!(*request, 1);
                if case == "route" {
                    return Err("route");
                }
                Ok(Reply {
                    lease,
                    fail: case == "send",
                    sent: sent.clone(),
                })
            },
            |request| {
                events.borrow_mut().push("dispatch");
                if case == "dispatch" {
                    return Err("dispatch");
                }
                Ok(request + 100)
            },
            &mut input[..input_len],
            &mut output[..output_len],
        );
        let expected = match case {
            "receive" => Err(ServeError::Receive(ReceiveError::Transport("receive"))),
            "capacity" => Err(ServeError::Receive(ReceiveError::Transport("capacity"))),
            "invalid" => Err(ServeError::Receive(ReceiveError::InvalidLength {
                reported: usize::MAX,
                capacity: 8,
            })),
            "truncated" | "trailing" => Err(ServeError::Receive(ReceiveError::Decode)),
            "route" => Err(ServeError::Route("route")),
            "dispatch" => Err(ServeError::Dispatch("dispatch")),
            "encode" => Err(ServeError::Send(SendError::Encode)),
            "send" => Err(ServeError::Send(SendError::Transport("send"))),
            _ => Ok(()),
        };
        assert_eq!(result, expected, "{case}");
        let expected_events: &[&str] = match case {
            "receive" | "capacity" => &["receive"],
            "invalid" | "truncated" | "trailing" => &["receive", "release"],
            "route" => &["receive", "route", "release"],
            "dispatch" | "encode" => &["receive", "route", "dispatch", "release"],
            _ => &["receive", "route", "dispatch", "send", "release"],
        };
        assert_eq!(&*events.borrow(), expected_events, "{case}");
        if case == "ok" {
            assert_eq!(*sent.borrow(), vec![vec![101, 0]]);
            assert_eq!(&output[2..], &[0xaa; 6]);
        } else {
            assert!(sent.borrow().is_empty());
        }
        drop(inbox);
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "release")
                .count(),
            1
        );
    }
}

#[test]
fn server_reuses_buffers_and_supports_borrowed_trait_object_endpoints() {
    let events = Events::default();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let mut inbox = Inbox {
        bytes: vec![2, 0],
        metadata: Some(Lease(events.clone())),
        events: events.clone(),
        fail: false,
        invalid_length: false,
    };
    let mut sender = Reply {
        lease: Lease(events.clone()),
        fail: false,
        sent: sent.clone(),
    };
    let mut input = [0; 8];
    let mut output = [0; 8];
    {
        let receiver: &mut dyn FrameReceiver<Error = &'static str, Metadata = Lease> = &mut inbox;
        let mut server = weave::Server::new(receiver, &mut input, &mut output);
        let target: &mut dyn FrameSender<Error = &'static str> = &mut sender;
        server
            .serve_once::<Query, _, (), ()>(|_, _| Ok(target), |value| Ok(value + 1))
            .unwrap();
        let _ = server.into_receiver();
    }
    assert_eq!(*sent.borrow(), vec![vec![3, 0]]);
    let mut server = weave::Server::new(inbox, &mut input, &mut output);
    assert!(server.receiver().metadata.is_none());
    server.receiver_mut().metadata = Some(Lease(events.clone()));
    server.receiver_mut().bytes = vec![4, 0];
    server
        .serve_once::<Query, _, (), ()>(|_, _| Ok(&mut sender), |value| Ok(value + 1))
        .unwrap();
    assert_eq!(*sent.borrow(), vec![vec![3, 0], vec![5, 0]]);
    let inbox = server.into_receiver();
    assert!(inbox.metadata.is_none());
    assert_eq!(&input[..2], &[4, 0]);
    assert_eq!(&output[..2], &[5, 0]);
}

#[cfg(feature = "derive")]
mod generated {
    use super::*;
    use weave::{Codec, EncodedSize};

    pub struct Number(u16);
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

    #[crate::renamed::interface(tag = u8, server)]
    #[weave(crate = crate::renamed)]
    mod api {
        use super::Number;
        #[operation(code = 1, request = Number, response = Number, handler = r#type)]
        pub struct Read;
        #[cfg_attr(all(), cfg_attr(all(), cfg(any())))]
        #[operation(code = 2, request = Missing, response = Missing, handler = hidden)]
        pub struct Hidden;
    }

    struct Handler {
        calls: usize,
    }
    impl api::Handler for Handler {
        type Error = &'static str;
        fn r#type(&mut self, request: u16) -> Result<Number, Self::Error> {
            self.calls += 1;
            Ok(Number(request + 1))
        }
    }

    #[test]
    fn generated_server_handles_decoded_types_aliases_cfg_and_handler_trait_objects() {
        let events = Events::default();
        let sent = Rc::new(RefCell::new(Vec::new()));
        let inbox = Inbox {
            bytes: vec![1, 42, 0],
            metadata: Some(Lease(events.clone())),
            events: events.clone(),
            fail: false,
            invalid_length: false,
        };
        let mut input = [0; 8];
        let mut output = [0; 8];
        let core = weave::Server::new(inbox, &mut input, &mut output);
        let mut server = api::Server::from_inner(core);
        let mut handler = Handler { calls: 0 };
        let erased: &mut dyn api::Handler<Error = &'static str> = &mut handler;
        server
            .serve_once(erased, |request, lease| {
                assert!(matches!(request, api::DecodedRequest::Read(42)));
                Ok::<_, ()>(Reply {
                    lease,
                    fail: false,
                    sent: sent.clone(),
                })
            })
            .unwrap();
        assert_eq!(handler.calls, 1);
        assert_eq!(*sent.borrow(), vec![vec![1, 43, 0]]);
        for malformed in [vec![99], vec![1, 42, 0, 7]] {
            server.inner_mut().receiver_mut().bytes = malformed;
            server.inner_mut().receiver_mut().metadata = Some(Lease(events.clone()));
            let result = server.serve_once(&mut handler, |_, _| -> Result<Reply, ()> {
                panic!("malformed request reached router")
            });
            assert_eq!(result, Err(ServeError::Receive(ReceiveError::Decode)));
        }
        assert_eq!(handler.calls, 1);
        assert!(server.into_inner().into_receiver().metadata.is_none());
    }

    #[cfg(feature = "alloc")]
    mod variable {
        use super::*;
        #[derive(weave::Frame)]
        pub struct Blob {
            #[frame(bytes(prefix = u8, max = 16))]
            data: Vec<u8>,
        }
        #[weave::interface(tag = u8, server)]
        mod echo {
            use super::Blob;
            #[operation(code = 1, request = Blob, response = Blob, handler = echo)]
            pub struct Echo;
        }
        struct Handler;
        impl echo::Handler for Handler {
            type Error = ();
            fn echo(&mut self, request: Blob) -> Result<Blob, ()> {
                Ok(request)
            }
        }
        #[test]
        fn variable_frames_reuse_server_storage_without_sending_stale_bytes() {
            let events = Events::default();
            let sent = Rc::new(RefCell::new(Vec::new()));
            let inbox = Inbox {
                bytes: vec![],
                metadata: None,
                events: events.clone(),
                fail: false,
                invalid_length: false,
            };
            let mut input = [0xaa; 32];
            let mut output = [0xaa; 32];
            let mut server = echo::Server::new(inbox, &mut input, &mut output);
            for length in [16, 0, 3] {
                let mut bytes = vec![1, length as u8];
                bytes.extend(vec![7; length]);
                server.inner_mut().receiver_mut().bytes = bytes.clone();
                server.inner_mut().receiver_mut().metadata = Some(Lease(events.clone()));
                server
                    .serve_once(&mut Handler, |_, lease| {
                        Ok::<_, ()>(Reply {
                            lease,
                            fail: false,
                            sent: sent.clone(),
                        })
                    })
                    .unwrap();
                assert_eq!(sent.borrow().last().unwrap(), &bytes);
                let decoded = echo::Response::decode_exact(&bytes).unwrap();
                assert!(
                    matches!(decoded, echo::DecodedResponse::Echo(Blob { data }) if data == vec![7; length])
                );
            }
            assert_eq!(sent.borrow().len(), 3);
        }
    }
}
