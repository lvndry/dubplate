//! Rendering a block of the deck on the audio thread.

use crate::jog::BlockStart;
use crate::keylock::KeyLockBlock;
use crate::processor::ProcessContext;
use crate::retire::RetireQueue;

use super::{AUDIBLE_SPEED, Deck, DeckRender, KEY_LOCK_TEMPO_TOLERANCE};

impl Deck {
    /// Renders `frame_count` frames into the deck's block, after the trim, the EQ, the filter
    /// and the inserts, and before the channel fader.
    pub(crate) fn render(
        &mut self,
        frame_count: usize,
        context: &ProcessContext,
        retire: &mut RetireQueue,
    ) -> DeckRender {
        let mut result = DeckRender::default();
        if let Some(frames) = self.platter.take_search() {
            self.jump_to_frame(self.voice.position + frames, retire);
        }
        let motor_step = if self.playing { self.rate } else { 0.0 };
        self.platter.begin_block(BlockStart {
            motor_step,
            frames: frame_count,
        });
        if std::mem::take(&mut self.pending_landing) {
            self.platter.drop_carry();
            self.land(retire);
        }
        let holding = self.platter.is_holding();
        let transport_target = if self.playing || holding { 1.0 } else { 0.0 };
        if self.voice.transport.target() != transport_target {
            self.voice.transport.set_target(transport_target);
        }
        self.block.set_frame_count(frame_count);
        let frame_count = self.block.frame_count();
        let mut advance = 0.0;
        for step in &mut self.steps[..frame_count] {
            *step = self.platter.next_step(motor_step);
            advance += *step;
        }
        let unit_step = self.source_rate() / self.output_rate;
        let audible_step = unit_step * AUDIBLE_SPEED;
        let end_frame = self.voice.end_frame();
        let active_loop = self.looping.active_loop();
        self.slip
            .advance(motor_step * frame_count as f64, end_frame);
        let tempo_moved = (self.tempo - 1.0).abs() > KEY_LOCK_TEMPO_TOLERANCE;
        let key_shifted = self.key_shift != 0.0;
        let tempo_semitones = if self.key_lock_enabled {
            0.0
        } else {
            12.0 * self.tempo.log2() as f32
        };
        self.key_lock
            .set_transpose(self.key_shift + tempo_semitones);
        let stretched = self.key_lock.prepare(
            KeyLockBlock {
                wanted: ((self.key_lock_enabled && tempo_moved) || key_shifted)
                    && self.playing
                    && !holding,
                track: self.voice.track.as_ref(),
                playhead: self.voice.position,
                advance,
                frames: frame_count,
                unit_step,
                active_loop,
            },
            retire,
            &mut result.starved,
        );
        let fully_stretched = stretched && self.key_lock.is_fully_in();
        let voice = &mut self.voice;
        let key_lock = &mut self.key_lock;
        let trim = &mut self.trim;
        let (left_channel, right_channel) = self.block.channels_mut();
        for (index, ((left, right), step)) in left_channel
            .iter_mut()
            .zip(right_channel.iter_mut())
            .zip(&self.steps[..frame_count])
            .enumerate()
        {
            let step = *step;
            let stylus_gain = (step.abs() / audible_step).min(1.0) as f32;
            let gain = voice.transport.next_value() * stylus_gain;
            let (playhead_left, playhead_right) = if stretched {
                key_lock.playhead_frame(index, voice, &mut result.starved)
            } else if gain > 0.0 {
                voice.read_playhead(&mut result.starved)
            } else {
                (0.0, 0.0)
            };
            let (tail_left, tail_right) = voice.next_tails(retire, &mut result.starved);
            if step != 0.0 {
                let previous = voice.position;
                voice.position = (previous + step).max(0.0);
                if let Some(wrapped) =
                    active_loop.and_then(|active| active.wrap(previous, voice.position))
                {
                    if fully_stretched {
                        voice.position = wrapped;
                    } else {
                        voice.jump(wrapped, step, retire);
                    }
                } else if let Some(end) = end_frame
                    && voice.position >= end
                {
                    voice.position = end;
                    if self.playing && !holding {
                        self.playing = false;
                        voice.transport.jump_to(0.0);
                    }
                }
            }
            let trim = trim.next_value();
            *left = (playhead_left * gain + tail_left) * trim;
            *right = (playhead_right * gain + tail_right) * trim;
        }
        self.strip.process(&mut self.block, context);
        self.pad_effects.process(&mut self.block, context);
        for processor in self.inserts.iter_mut().flatten() {
            processor.process(&mut self.block, context);
        }
        result
    }
}
