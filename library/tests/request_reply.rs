#![cfg(feature = "derive")]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use weave::correlation::CheckedReceiver;
use weave::request_reply;
use weave::{FrameReceiver, FrameSender, ReceivedFrame};

#[derive(weave::Frame)]
pub struct ReadRequest {
    reply: u32,
    address: u16,
}

#[weave::interface(tag = u8, client, server)]
mod api {
    use super::ReadRequest;
    #[operation(code = 7, request = ReadRequest, response = u16, call = read, handler = read)]
    pub struct Read;
    #[operation(code = 8, request = ReadRequest, response = u16, call = status, handler = status)]
    pub struct Status;
}

struct Service;
impl api::Handler for Service {
    type Error = &'static str;
    fn read(&mut self, request: ReadRequest) -> Result<u16, Self::Error> {
        request.address.checked_add(100).ok_or("overflow")
    }
    fn status(&mut self, _: ReadRequest) -> Result<u16, Self::Error> {
        Ok(1)
    }
}

type Queue = Rc<RefCell<VecDeque<(Vec<u8>, u32)>>>;

struct Server {
    replies: Queue,
    received: Vec<Vec<u8>>,
}

impl FrameSender for Server {
    type Error = &'static str;
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let mut request_buffer = [0; 7];
        let mut response_buffer = [0; 3];
        let mut server = api::Server::new(
            RequestFrame(bytes),
            &mut request_buffer,
            &mut response_buffer,
        );
        server
            .serve_once(&mut Service, |request, ()| {
                let id = match request {
                    api::DecodedRequest::Read(value) | api::DecodedRequest::Status(value) => {
                        value.reply
                    }
                };
                Ok::<_, &'static str>(ReplyTo {
                    queue: self.replies.clone(),
                    id,
                })
            })
            .map_err(|_| "serve")?;
        self.received.push(bytes.to_vec());
        Ok(())
    }
}

// The in-memory request path is dedicated to this caller.
struct RequestFrame<'a>(&'a [u8]);
impl FrameReceiver for RequestFrame<'_> {
    type Error = &'static str;
    type Metadata = ();
    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<()>, Self::Error> {
        out.get_mut(..self.0.len())
            .ok_or("capacity")?
            .copy_from_slice(self.0);
        Ok(ReceivedFrame {
            len: self.0.len(),
            metadata: (),
        })
    }
}

struct ReplyTo {
    queue: Queue,
    id: u32,
}
impl FrameSender for ReplyTo {
    type Error = &'static str;
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.queue.borrow_mut().push_back((bytes.to_vec(), self.id));
        Ok(())
    }
}

struct Inbox(Queue);
impl FrameReceiver for Inbox {
    type Error = &'static str;
    type Metadata = u32;
    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<u32>, Self::Error> {
        let (bytes, id) = self.0.borrow_mut().pop_front().ok_or("empty")?;
        out.get_mut(..bytes.len())
            .ok_or("capacity")?
            .copy_from_slice(&bytes);
        Ok(ReceivedFrame {
            len: bytes.len(),
            metadata: id,
        })
    }
}

#[test]
fn generated_interface_dispatch_and_request_reply_work_end_to_end() {
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let mut server = Server {
        replies: queue.clone(),
        received: Vec::new(),
    };
    for reply_id in [1u32, 2, 3] {
        // Prepare the route before constructing a request that carries it.
        let mut receiver =
            CheckedReceiver::new(Inbox(queue.clone()), move |id: &u32, bytes: &[u8]| {
                if *id == reply_id && bytes.first() == Some(&7) {
                    Ok(())
                } else {
                    Err("unassociated reply")
                }
            });
        let request = api::Request::Read(ReadRequest {
            reply: reply_id,
            address: 42,
        });
        let result = request_reply::call::<api::Interface, _, _>(
            &mut server,
            &mut receiver,
            &request,
            &mut [0; 64],
            &mut [0; 64],
        )
        .unwrap();
        assert!(matches!(result.value, api::DecodedResponse::Read(142)));
        assert_eq!(result.metadata, reply_id);
    }
    assert_eq!(server.received.len(), 3);
    assert_eq!(server.received[0], vec![7, 1, 0, 0, 0, 42, 0]);
    assert!(queue.borrow().is_empty());
    assert_eq!(api::Read::TAG, api::Operation::Read);
}

#[test]
fn typed_operation_wraps_request_and_extracts_response() {
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let mut server = Server {
        replies: queue.clone(),
        received: Vec::new(),
    };
    let mut receiver = CheckedReceiver::new(Inbox(queue.clone()), |id: &u32, _: &[u8]| {
        if *id == 17 {
            Ok(())
        } else {
            Err("wrong route")
        }
    });
    let received = request_reply::call_operation::<api::Read, _, _>(
        &mut server,
        &mut receiver,
        ReadRequest {
            reply: 17,
            address: 42,
        },
        &mut [0; 64],
        &mut [0; 64],
    )
    .unwrap();
    assert_eq!(received.value, 142u16);
    assert_eq!(received.metadata, 17);
    assert_eq!(server.received, vec![vec![7, 17, 0, 0, 0, 42, 0]]);
    assert!(queue.borrow().is_empty());
}

#[test]
fn associated_reply_with_another_operation_is_rejected_without_retry() {
    let queue = Rc::new(RefCell::new(VecDeque::from([(vec![8, 1, 0], 17)])));
    let mut server = Server {
        replies: queue.clone(),
        received: Vec::new(),
    };
    let mut receiver = CheckedReceiver::new(Inbox(queue.clone()), |id: &u32, _: &[u8]| {
        if *id == 17 {
            Ok(())
        } else {
            Err("wrong route")
        }
    });
    let mut request_buffer = [0; 64];
    let mut response_buffer = [0; 64];
    let mut client = api::Client::new(&mut server, &mut request_buffer, &mut response_buffer);
    let result = client.read(
        &mut receiver,
        ReadRequest {
            reply: 17,
            address: 42,
        },
    );
    assert!(matches!(
        result,
        Err(request_reply::OperationError::WrongOperation)
    ));
    assert_eq!(server.received.len(), 1);
    assert_eq!(queue.borrow().len(), 1);
}

#[test]
fn generated_client_reuses_storage_with_a_fresh_association_per_call() {
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let server = Server {
        replies: queue.clone(),
        received: Vec::new(),
    };
    let mut request_buffer = [0xaa; 64];
    let mut response_buffer = [0xaa; 64];
    let core = weave::Client::new(server, &mut request_buffer, &mut response_buffer);
    let mut client = api::Client::from_inner(core);
    for id in [1u32, 2, 3] {
        let mut reply =
            CheckedReceiver::new(Inbox(queue.clone()), move |actual: &u32, _: &[u8]| {
                if *actual == id {
                    Ok(())
                } else {
                    Err("wrong route")
                }
            });
        let request = ReadRequest {
            reply: id,
            address: 42,
        };
        let received = if id == 2 {
            client.status(&mut reply, request)
        } else {
            client.read(&mut reply, request)
        }
        .unwrap();
        assert_eq!(received.value, if id == 2 { 1 } else { 142 });
        assert_eq!(received.metadata, id);
        assert!(queue.borrow().is_empty());
    }
    assert_eq!(client.inner_mut().sender().received.len(), 3);
    let server = client.into_inner().into_sender();
    assert_eq!(
        server.received,
        vec![
            vec![7, 1, 0, 0, 0, 42, 0],
            vec![8, 2, 0, 0, 0, 42, 0],
            vec![7, 3, 0, 0, 0, 42, 0],
        ]
    );
    assert_eq!(&request_buffer[7..], &[0xaa; 57]);
    assert_eq!(&response_buffer[3..], &[0xaa; 61]);
}
