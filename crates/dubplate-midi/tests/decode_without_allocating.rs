//! Decoding runs on the MIDI thread, where allocating could stall the jog wheels. This test
//! counts every allocation made while decoding.
//!
//! Counting is armed per thread, so allocations by the test harness or other threads are not
//! counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use dubplate_midi::{Button, Deck, Decoder, Feedback, Layer, Mapping};

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every call is forwarded unchanged to the system allocator; the counters are
// const-initialised thread locals, which never allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, which `System` shares.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `System.alloc` above with the same layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract, which `System` shares.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn decoding_and_encoding_never_allocate() {
    let mapping = Arc::new(Mapping::built_in_for_port("DDJ-FLX4").unwrap().unwrap());
    let mut decoder = Decoder::new(Arc::clone(&mapping));
    let messages: [[u8; 3]; 8] = [
        [0x90, 0x0B, 0x7F],
        [0xB0, 0x22, 0x41],
        [0xB0, 0x13, 0x40],
        [0xB0, 0x33, 0x00],
        [0x97, 0x05, 0x7F],
        [0xB6, 0x40, 0x7F],
        [0x96, 0x6D, 0x00],
        [0x92, 0x00, 0x7F],
    ];
    let light = Feedback::Light {
        button: Button::Play(Deck::A),
        layer: Layer::Normal,
        on: true,
    };

    let before = ALLOCATIONS.with(Cell::get);
    COUNTING.with(|flag| flag.set(true));
    let mut decoded = 0;
    for _ in 0..1000 {
        for message in &messages {
            decoded += usize::from(decoder.decode(message).is_some());
        }
        decoded += usize::from(mapping.encode(light).is_some());
    }
    COUNTING.with(|flag| flag.set(false));
    let after = ALLOCATIONS.with(Cell::get);

    assert_eq!(decoded, 7000);
    assert_eq!(after - before, 0, "decoding allocated");
}
