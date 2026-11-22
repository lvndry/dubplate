//! A track's cues as a DJ uses them: a hot cue on each of the pads A to H that has one, and the
//! memory cues in time order. Any cue can be a saved loop, and each keeps rekordbox's colour
//! and the DJ's name for it.

use serde::Serialize;

use crate::cue_colors::{MemoryCueColor, PaletteColor};
use crate::model::{Cue, CueKind, HotCuePad};

/// How many memory cues a CDJ shows and calls for a track.
pub const MEMORY_CUE_LIMIT: usize = 10;

/// A hot cue, or a hot loop when it has an end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotCue {
    /// Where it is, in milliseconds from the start of the track.
    pub start_ms: u32,
    /// Where the loop ends, after `start_ms`, for a hot loop.
    pub loop_end_ms: Option<u32>,
    /// Its own colour, or `None` to be drawn in its pad's default.
    pub color: Option<PaletteColor>,
    /// The name the DJ gave it, empty when it has none.
    pub name: String,
}

/// A memory cue, or a saved memory loop when it has an end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryCue {
    /// Where it is, in milliseconds from the start of the track.
    pub start_ms: u32,
    /// Where the loop ends, after `start_ms`, for a saved loop.
    pub loop_end_ms: Option<u32>,
    /// Its colour, or `None` to be drawn red.
    pub color: Option<MemoryCueColor>,
    /// The name the DJ gave it, empty when it has none.
    pub name: String,
}

impl HotCue {
    /// A hot cue at `start_ms` with no loop, colour or name, as a pad sets one.
    pub fn at(start_ms: u32) -> Self {
        Self {
            start_ms,
            loop_end_ms: None,
            color: None,
            name: String::new(),
        }
    }

    /// Where it is, in seconds.
    pub fn start_seconds(&self) -> f64 {
        seconds(self.start_ms)
    }

    /// The colour it is drawn in: its own, else the default rekordbox draws for a hot loop or
    /// for a hot cue on `pad`.
    pub fn display_color(&self, pad: HotCuePad) -> PaletteColor {
        self.color.unwrap_or(match self.loop_end_ms {
            Some(_) => PaletteColor::HOT_LOOP_DEFAULT,
            None => PaletteColor::pad_default(pad),
        })
    }
}

impl MemoryCue {
    /// A memory cue at `start_ms` with no loop, colour or name.
    pub fn at(start_ms: u32) -> Self {
        Self {
            start_ms,
            loop_end_ms: None,
            color: None,
            name: String::new(),
        }
    }

    /// Where it is, in seconds.
    pub fn start_seconds(&self) -> f64 {
        seconds(self.start_ms)
    }
}

/// The cues of one track.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TrackCues {
    hot: [Option<HotCue>; HotCuePad::ALL.len()],
    memory: Vec<MemoryCue>,
}

impl TrackCues {
    /// The cues `hot` puts on pads A to H in order, and the memory cues `memory`, which are
    /// kept in time order.
    pub fn new(hot: [Option<HotCue>; HotCuePad::ALL.len()], mut memory: Vec<MemoryCue>) -> Self {
        memory.sort_by_key(|cue| cue.start_ms);
        Self { hot, memory }
    }

    /// The cues of one track as `master.db` holds them. A second cue on a pad, a cue of a kind
    /// this version does not know, and a loop end not after its start are left out.
    pub fn from_rekordbox<'a>(cues: impl IntoIterator<Item = &'a Cue>) -> Self {
        let mut hot: [Option<HotCue>; HotCuePad::ALL.len()] = Default::default();
        let mut memory = Vec::new();
        for cue in cues {
            let loop_end_ms = cue.loop_end_ms.filter(|end| *end > cue.start_ms);
            let name = cue.comment.clone().unwrap_or_default();
            match cue.kind {
                CueKind::Hot { pad } => {
                    let slot = &mut hot[pad.index()];
                    if slot.is_none() {
                        *slot = Some(HotCue {
                            start_ms: cue.start_ms,
                            loop_end_ms,
                            color: cue.hot_cue_color,
                            name,
                        });
                    }
                }
                CueKind::Memory => memory.push(MemoryCue {
                    start_ms: cue.start_ms,
                    loop_end_ms,
                    color: cue.memory_color,
                    name,
                }),
                CueKind::Unknown { .. } => {}
            }
        }
        Self::new(hot, memory)
    }

    /// Whether the track has no cue at all.
    pub fn is_empty(&self) -> bool {
        self.memory.is_empty() && self.hot.iter().all(Option::is_none)
    }

    /// The hot cue on `pad`, if it has one.
    pub fn hot_cue(&self, pad: HotCuePad) -> Option<&HotCue> {
        self.hot[pad.index()].as_ref()
    }

    /// Every hot cue with its pad, A first.
    pub fn hot_cues(&self) -> impl Iterator<Item = (HotCuePad, &HotCue)> {
        HotCuePad::ALL
            .into_iter()
            .zip(&self.hot)
            .filter_map(|(pad, cue)| Some((pad, cue.as_ref()?)))
    }

    /// Puts `cue` on `pad`, in place of the one there.
    pub fn set_hot_cue(&mut self, pad: HotCuePad, cue: HotCue) {
        self.hot[pad.index()] = Some(cue);
    }

    /// Clears `pad`.
    pub fn delete_hot_cue(&mut self, pad: HotCuePad) {
        self.hot[pad.index()] = None;
    }

    /// The memory cues and saved memory loops, in time order.
    pub fn memory_cues(&self) -> &[MemoryCue] {
        &self.memory
    }

    /// The memory cues from `start_ms` to `end_ms`, both included, in time order.
    pub fn memory_cues_between(&self, start_ms: u32, end_ms: u32) -> &[MemoryCue] {
        let first = self.memory.partition_point(|cue| cue.start_ms < start_ms);
        let last = self.memory.partition_point(|cue| cue.start_ms <= end_ms);
        &self.memory[first..last.max(first)]
    }

    /// Adds a memory cue in its place in time, after any already at the same place.
    pub fn insert_memory_cue(&mut self, cue: MemoryCue) {
        let place = self
            .memory
            .partition_point(|existing| existing.start_ms <= cue.start_ms);
        self.memory.insert(place, cue);
    }

    /// Removes every memory cue `remove` picks.
    pub fn remove_memory_cues(&mut self, mut remove: impl FnMut(&MemoryCue) -> bool) {
        self.memory.retain(|cue| !remove(cue));
    }

    /// Every saved loop, memory and hot, as its start and end in milliseconds.
    pub fn loops(&self) -> impl Iterator<Item = (u32, u32)> {
        let memory = self
            .memory
            .iter()
            .filter_map(|cue| Some((cue.start_ms, cue.loop_end_ms?)));
        let hot = self
            .hot
            .iter()
            .flatten()
            .filter_map(|cue| Some((cue.start_ms, cue.loop_end_ms?)));
        memory.chain(hot)
    }
}

/// `seconds` from the start of a track in whole milliseconds, as rekordbox keeps cues: the
/// nearest one, never before the start.
pub fn cue_milliseconds(seconds: f64) -> u32 {
    let milliseconds = (seconds * 1000.0).round();
    if milliseconds.is_nan() || milliseconds <= 0.0 {
        0
    } else if milliseconds >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        milliseconds as u32
    }
}

fn seconds(milliseconds: u32) -> f64 {
    f64::from(milliseconds) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(kind: CueKind, start_ms: u32, loop_end_ms: Option<u32>) -> Cue {
        Cue {
            id: String::new(),
            track_id: "t1".to_owned(),
            kind,
            start_ms,
            loop_end_ms,
            hot_cue_color: None,
            memory_color: None,
            comment: None,
        }
    }

    #[test]
    fn rekordbox_cues_go_to_their_pads_and_memory_cues_into_time_order() {
        let mut named = cue(CueKind::Hot { pad: HotCuePad::C }, 4_000, Some(6_000));
        named.comment = Some("Drop".to_owned());
        named.hot_cue_color = PaletteColor::from_index(42);
        let mut blue = cue(CueKind::Memory, 9_000, None);
        blue.memory_color = Some(MemoryCueColor::Blue);
        let cues = TrackCues::from_rekordbox(&[
            blue,
            named,
            cue(CueKind::Memory, 2_000, Some(2_000)),
            cue(CueKind::Hot { pad: HotCuePad::C }, 8_000, None),
            cue(CueKind::Unknown { kind: 4 }, 1_000, None),
        ]);
        let drop = cues.hot_cue(HotCuePad::C).unwrap();
        assert_eq!(drop.start_ms, 4_000, "the first cue on a pad wins");
        assert_eq!(drop.loop_end_ms, Some(6_000));
        assert_eq!(drop.name, "Drop");
        assert_eq!(drop.color.map(PaletteColor::index), Some(42));
        let memory: Vec<(u32, Option<u32>)> = cues
            .memory_cues()
            .iter()
            .map(|cue| (cue.start_ms, cue.loop_end_ms))
            .collect();
        assert_eq!(memory, [(2_000, None), (9_000, None)]);
        assert_eq!(cues.memory_cues()[1].color, Some(MemoryCueColor::Blue));
        assert_eq!(cues.loops().collect::<Vec<_>>(), [(4_000, 6_000)]);
    }

    #[test]
    fn hot_cues_are_set_and_deleted_by_pad() {
        let mut cues = TrackCues::default();
        assert!(cues.is_empty());
        cues.set_hot_cue(HotCuePad::H, HotCue::at(12_500));
        cues.set_hot_cue(HotCuePad::A, HotCue::at(1_000));
        let pads: Vec<(char, u32)> = cues
            .hot_cues()
            .map(|(pad, cue)| (pad.letter(), cue.start_ms))
            .collect();
        assert_eq!(pads, [('A', 1_000), ('H', 12_500)]);
        cues.delete_hot_cue(HotCuePad::H);
        assert_eq!(cues.hot_cue(HotCuePad::H), None);
        assert!(!cues.is_empty());
    }

    #[test]
    fn memory_cues_stay_in_time_order() {
        let mut cues = TrackCues::new(
            Default::default(),
            vec![MemoryCue::at(30_000), MemoryCue::at(10_000)],
        );
        cues.insert_memory_cue(MemoryCue::at(20_000));
        cues.insert_memory_cue(MemoryCue::at(40_000));
        let starts: Vec<u32> = cues.memory_cues().iter().map(|cue| cue.start_ms).collect();
        assert_eq!(starts, [10_000, 20_000, 30_000, 40_000]);
        let between: Vec<u32> = cues
            .memory_cues_between(20_000, 35_000)
            .iter()
            .map(|cue| cue.start_ms)
            .collect();
        assert_eq!(between, [20_000, 30_000]);
        assert!(cues.memory_cues_between(41_000, 90_000).is_empty());
        assert!(cues.memory_cues_between(30_000, 10_000).is_empty());
        cues.remove_memory_cues(|cue| cue.start_ms == 20_000);
        assert_eq!(cues.memory_cues().len(), 3);
    }

    #[test]
    fn a_cue_without_a_colour_is_drawn_in_its_pad_s_default() {
        let plain = HotCue::at(0);
        assert_eq!(
            plain.display_color(HotCuePad::A),
            PaletteColor::pad_default(HotCuePad::A)
        );
        let hot_loop = HotCue {
            loop_end_ms: Some(2_000),
            ..HotCue::at(0)
        };
        assert_eq!(
            hot_loop.display_color(HotCuePad::A),
            PaletteColor::HOT_LOOP_DEFAULT
        );
        let own = HotCue {
            color: PaletteColor::from_index(5),
            ..HotCue::at(0)
        };
        assert_eq!(own.display_color(HotCuePad::B).index(), 5);
    }

    #[test]
    fn seconds_become_whole_milliseconds() {
        assert_eq!(cue_milliseconds(12.3456), 12_346);
        assert_eq!(cue_milliseconds(-1.0), 0);
        assert_eq!(cue_milliseconds(f64::NAN), 0);
        assert_eq!(cue_milliseconds(1e12), u32::MAX);
    }
}
