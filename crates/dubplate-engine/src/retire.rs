//! Objects the audio thread lets go of, sent back to the control side to be freed.

use std::sync::Arc;

use rtrb::Producer;

use crate::grid::BeatGrid;
use crate::processor::AudioProcessor;
use crate::recording::RecordingSink;
use crate::track::TrackBuffer;

/// Something the audio thread no longer needs. It goes back to the control side to be dropped,
/// because freeing memory on the audio thread can block.
pub(crate) enum Retired {
    Track(#[allow(dead_code)] Arc<TrackBuffer>),
    Processor(#[allow(dead_code)] Box<dyn AudioProcessor>),
    BeatGrid(#[allow(dead_code)] BeatGrid),
    Recording(#[allow(dead_code)] RecordingSink),
}

/// The audio thread's end of the queue of retired objects.
pub(crate) struct RetireQueue {
    producer: Producer<Retired>,
}

impl RetireQueue {
    pub(crate) fn new(producer: Producer<Retired>) -> Self {
        Self { producer }
    }

    pub(crate) fn track(&mut self, track: Arc<TrackBuffer>) {
        self.retire(Retired::Track(track));
    }

    pub(crate) fn beat_grid(&mut self, beat_grid: BeatGrid) {
        self.retire(Retired::BeatGrid(beat_grid));
    }

    pub(crate) fn recording(&mut self, sink: RecordingSink) {
        self.retire(Retired::Recording(sink));
    }

    pub(crate) fn processor(&mut self, processor: Box<dyn AudioProcessor>) {
        self.retire(Retired::Processor(processor));
    }

    fn retire(&mut self, item: Retired) {
        if let Err(rtrb::PushError::Full(item)) = self.producer.push(item) {
            // Leaking is the lesser harm: freeing here could block the audio thread.
            std::mem::forget(item);
        }
    }
}

/// Puts a processor in a slot and sends the one it replaces off the audio thread. A slot that
/// does not exist sends the new processor back too.
pub(crate) fn replace_insert(
    slot: Option<&mut Option<Box<dyn AudioProcessor>>>,
    processor: Option<Box<dyn AudioProcessor>>,
    retire: &mut RetireQueue,
) {
    let unused = match slot {
        Some(slot) => std::mem::replace(slot, processor),
        None => processor,
    };
    if let Some(unused) = unused {
        retire.processor(unused);
    }
}
