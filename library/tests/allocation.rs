#![cfg(feature = "alloc")]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};
use weave::{Codec, EncodedSize, codec::bounded};

// This integration-test executable alone replaces the allocator. The
// nonallocating, non-dropping TLS flag isolates fault injection to its caller.
thread_local! {
    static REMAINING: Cell<Option<usize>> = const { Cell::new(None) };
}

fn reject() -> bool {
    REMAINING
        .try_with(|remaining| match remaining.get() {
            Some(0) => {
                remaining.set(None);
                true
            }
            Some(count) => {
                remaining.set(Some(count - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}

struct FaultAllocator;

#[global_allocator]
static ALLOCATOR: FaultAllocator = FaultAllocator;

// SAFETY: All successful operations use the System allocator with the original
// layout. Returning null does not allocate or invalidate the original block.
unsafe impl GlobalAlloc for FaultAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if reject() {
            ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if reject() {
            ptr::null_mut()
        } else {
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if reject() {
            ptr::null_mut()
        } else {
            unsafe { System.realloc(block, layout, size) }
        }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        unsafe { System.dealloc(block, layout) }
    }
}

fn arm(successes: usize) {
    REMAINING.with(|remaining| remaining.set(Some(successes)));
}

fn disarm() -> bool {
    REMAINING.with(|remaining| remaining.take().is_none())
}

static DECODED: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);

struct Tracked(u8);

impl Drop for Tracked {
    fn drop(&mut self) {
        DROPPED.fetch_add(usize::from(self.0), Ordering::Relaxed);
    }
}

impl Codec for Tracked {
    type Decoded = Self;
    const SIZE: EncodedSize = EncodedSize::Fixed(1);

    fn encode(&self, out: &mut [u8]) -> Option<usize> {
        self.0.encode(out)
    }

    fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
        let value = *bytes.first()?;
        DECODED.fetch_add(1, Ordering::Relaxed);
        Some((Self(value), 1))
    }
}

#[test]
fn owned_decoders_fail_without_leaking_partial_values_when_reservation_fails() {
    arm(0);
    let bytes = bounded::decode_owned_bytes::<u8>(&[3, 1, 2, 3], 3);
    let rejected = disarm();
    assert!(rejected);
    assert!(bytes.is_none());

    arm(0);
    let text = bounded::decode_owned_text::<u8>(b"\x03abc", 3);
    let rejected = disarm();
    assert!(rejected);
    assert!(text.is_none());

    // Allow the first reservation, then fail the next allocation or growth.
    // The count is finite and each decoded element occupies one byte.
    let mut input = [1u8; 65];
    input[0] = 64;
    arm(1);
    let sequence = bounded::decode_sequence::<u8, Tracked>(&input, 64);
    let rejected = disarm();
    assert!(rejected);
    assert!(sequence.is_none());
    let decoded = DECODED.load(Ordering::Relaxed);
    assert!(decoded > 1);
    assert!(decoded <= 64);
    assert_eq!(DROPPED.load(Ordering::Relaxed), decoded);
}
