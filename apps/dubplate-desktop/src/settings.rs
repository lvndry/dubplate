//! What Dubplate remembers between launches, in `settings.toml` next to the rekordbox key:
//! `~/Library/Application Support/dubplate/` on macOS and `~/.config/dubplate/` on Linux.

use std::path::{Path, PathBuf};

use dubplate_engine::{
    ChannelFaderCurve, Crossfader, CrossfaderAssign, CrossfaderCurve, DECK_COUNT, DeckId,
};
use dubplate_library::{DataFolder, LibrarySettings};
use dubplate_stems::StemsLocation;
use serde::{Deserialize, Serialize};

use crate::commands::Mode;
use crate::controls::{DEFAULT_MASTER_LEVEL, DEFAULT_SAMPLER_VOLUME, MixerPositions};
use crate::effects::EffectUnitSetting;
use crate::library_view::SortSetting;
use crate::pad_fx::PadFxSetting;

/// Why the settings could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The system has no configuration directory for this user.
    #[error("there is no configuration directory for this user")]
    NoConfigurationDirectory,
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The settings file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The file is not valid settings. A copy is kept before anything can overwrite it.
    #[error("{path} is not valid, a copy is kept at {backup}: {source}")]
    Parse {
        /// The settings file.
        path: PathBuf,
        /// Where the unreadable file was copied.
        backup: PathBuf,
        /// What is wrong.
        source: Box<toml::de::Error>,
    },
    /// The settings could not be written as TOML.
    #[error("the settings could not be written: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Everything Dubplate remembers. Settings missing from the file take their default, so a file
/// from an older version still loads, and settings this version does not know are ignored, so
/// going back a version keeps the DJ's folders, sound card and the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Whether a playing deck refuses a new track.
    pub lock_playing_deck: bool,
    /// Whether cues, loops and jumps land on the beat grid.
    pub quantize: bool,
    /// Whether each deck's jog top holds the record, deck A first.
    pub vinyl_mode: [bool; DECK_COUNT],
    /// The sound card last used, by name.
    pub audio_device: Option<String>,
    /// The music folders, whether rekordbox is in the library, and the relinked files.
    #[serde(flatten)]
    pub library: LibrarySettings,
    /// Each effect unit's effect, target, beat and depth, FX 1 first.
    pub effect_units: Vec<EffectUnitSetting>,
    /// The Smart CFX preset SMART CFX turns on, by name.
    pub smart_cfx_preset: Option<String>,
    /// Whether SMART CFX was on, so the channels' CFX knobs play the preset from launch.
    pub smart_cfx_on: bool,
    /// The sampler's volume and the sample in each slot.
    pub sampler: SamplerSetting,
    /// The 32 Pad FX pads and the preset they started from.
    pub pad_fx: PadFxSetting,
    /// The screen Dubplate was on, `prepare` or `perform`.
    pub screen: Option<String>,
    /// The master and headphone levels.
    pub levels: LevelsSetting,
    /// Where a track is cued when it loads.
    pub auto_cue: AutoCueSetting,
    /// The fader curves and the crossfader assign switches.
    pub mixer: MixerSetting,
    /// How the library's tracks are sorted on each screen.
    pub library_sort: LibrarySortSetting,
    /// Whether a track loaded on a deck starts with key lock on, as rekordbox's Master Tempo
    /// default: the deck's Key lock button still changes it for the track on the deck.
    pub key_lock_on_new_tracks: bool,
}

/// Where a track is cued when it loads, as rekordbox's Preferences > Controller > Deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoCueSetting {
    /// Load Setting: cue at the track's earliest memory or hot cue when it has one.
    pub saved_cues_first: bool,
    /// Auto Cue: otherwise cue at the first sound, on the beat it starts with.
    pub first_sound: bool,
}

impl Default for AutoCueSetting {
    fn default() -> Self {
        Self {
            saved_cues_first: true,
            first_sound: true,
        }
    }
}

/// The mixer's curves and switches as the DJ left them, by name: `blend`, `constant` or
/// `scratch` for the crossfader curve, `slow`, `standard` or `fast` for the channel faders,
/// and `A`, `THRU` or `B` for each deck's crossfader assign, deck A first. A name this
/// version does not know takes the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerSetting {
    /// Whether the crossfader changes the sound; off, both channels play as if on THRU.
    pub crossfader_on: bool,
    /// The crossfader curve.
    pub crossfader_curve: String,
    /// The channel fader curve.
    pub channel_fader_curve: String,
    /// Each deck's side of the crossfader.
    pub crossfader_assign: [String; DECK_COUNT],
}

impl Default for MixerSetting {
    fn default() -> Self {
        Self::of(&Crossfader::default(), ChannelFaderCurve::default())
    }
}

impl MixerSetting {
    /// The setting that keeps `crossfader`'s switches and curve, its position aside, and
    /// the channel fader curve.
    pub fn of(crossfader: &Crossfader, channel_fader_curve: ChannelFaderCurve) -> Self {
        Self {
            crossfader_on: crossfader.enabled,
            crossfader_curve: crossfader.curve.to_string(),
            channel_fader_curve: channel_fader_curve.to_string(),
            crossfader_assign: crossfader.assign.map(|assign| assign.to_string()),
        }
    }

    /// The crossfader curve kept, or the default.
    pub fn crossfader_curve(&self) -> CrossfaderCurve {
        named(&CrossfaderCurve::ALL, &self.crossfader_curve).unwrap_or_default()
    }

    /// The channel fader curve kept, or the default.
    pub fn channel_fader_curve(&self) -> ChannelFaderCurve {
        named(&ChannelFaderCurve::ALL, &self.channel_fader_curve).unwrap_or_default()
    }

    /// The side kept for `deck`, or its default side.
    pub fn crossfader_assign(&self, deck: DeckId) -> CrossfaderAssign {
        named(
            &CrossfaderAssign::ALL,
            &self.crossfader_assign[deck.index()],
        )
        .unwrap_or_else(|| CrossfaderAssign::default_for(deck))
    }
}

/// The value of `values` written as `name`.
fn named<T: Copy + std::fmt::Display>(values: &[T], name: &str) -> Option<T> {
    values
        .iter()
        .copied()
        .find(|value| value.to_string() == name)
}

/// The output levels as the DJ left them, so Dubplate never opens louder than it closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelsSetting {
    /// The master level, from 0.0 to 1.0.
    pub master: f32,
    /// The headphone level, from 0.0 to 1.0.
    pub headphones: f32,
    /// The headphone mix between the cued channels and the master, from 0.0 to 1.0.
    pub headphone_mix: f32,
}

impl Default for LevelsSetting {
    fn default() -> Self {
        let defaults = MixerPositions::default();
        Self {
            master: DEFAULT_MASTER_LEVEL,
            headphones: defaults.headphone_level,
            headphone_mix: defaults.headphone_mix,
        }
    }
}

impl LevelsSetting {
    /// The levels of `positions`.
    pub fn of(positions: &MixerPositions) -> Self {
        Self {
            master: positions.master_level,
            headphones: positions.headphone_level,
            headphone_mix: positions.headphone_mix,
        }
    }

    /// `positions` with these levels, each kept between 0.0 and 1.0.
    pub fn apply(&self, positions: MixerPositions) -> MixerPositions {
        MixerPositions {
            master_level: self.master.clamp(0.0, 1.0),
            headphone_level: self.headphones.clamp(0.0, 1.0),
            headphone_mix: self.headphone_mix.clamp(0.0, 1.0),
            ..positions
        }
    }
}

/// The name the settings keep a screen by.
fn screen_name(screen: Mode) -> &'static str {
    match screen {
        Mode::Prepare => "prepare",
        Mode::Perform => "perform",
    }
}

impl Settings {
    /// The screen to open on: the one Dubplate was on, Perform the first time.
    pub fn screen(&self) -> Mode {
        [Mode::Prepare, Mode::Perform]
            .into_iter()
            .find(|screen| self.screen.as_deref() == Some(screen_name(*screen)))
            .unwrap_or(Mode::Perform)
    }

    /// Remembers the screen Dubplate is on.
    pub fn set_screen(&mut self, screen: Mode) {
        self.screen = Some(screen_name(screen).to_owned());
    }
}

/// How the library's tracks are sorted on each screen, when the DJ sorted them. A screen
/// without one shows each crate in its own order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySortSetting {
    /// In Prepare.
    pub prepare: Option<SortSetting>,
    /// In Perform.
    pub perform: Option<SortSetting>,
}

impl LibrarySortSetting {
    /// The sort of `screen`.
    pub fn of(&self, screen: Mode) -> Option<&SortSetting> {
        match screen {
            Mode::Prepare => self.prepare.as_ref(),
            Mode::Perform => self.perform.as_ref(),
        }
    }

    /// Keeps `sort` for `screen`.
    pub fn set(&mut self, screen: Mode, sort: Option<SortSetting>) {
        match screen {
            Mode::Prepare => self.prepare = sort,
            Mode::Perform => self.perform = sort,
        }
    }
}

/// The sampler as the DJ left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SamplerSetting {
    /// The sampler's volume, from 0.0 to 1.0.
    pub volume: f32,
    /// The loaded slots; the others are empty.
    pub slots: Vec<SampleSlotSetting>,
}

impl Default for SamplerSetting {
    fn default() -> Self {
        Self {
            volume: DEFAULT_SAMPLER_VOLUME,
            slots: Vec::new(),
        }
    }
}

/// One sampler slot as the DJ left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleSlotSetting {
    /// The slot, 0 for slot 1.
    pub slot: usize,
    /// The sample's audio file.
    pub path: PathBuf,
    /// The sample's own tempo, when known, for sync.
    #[serde(default)]
    pub bpm: Option<f64>,
    /// Plays round and round until stopped, rather than once.
    #[serde(default)]
    pub looped: bool,
    /// Plays only while its pad is held.
    #[serde(default)]
    pub gate: bool,
    /// Plays at the tempo master's BPM.
    #[serde(default)]
    pub sync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lock_playing_deck: true,
            quantize: true,
            vinyl_mode: [true; DECK_COUNT],
            audio_device: None,
            library: LibrarySettings::default(),
            effect_units: Vec::new(),
            smart_cfx_preset: None,
            smart_cfx_on: false,
            sampler: SamplerSetting::default(),
            pad_fx: PadFxSetting::default(),
            screen: None,
            levels: LevelsSetting::default(),
            auto_cue: AutoCueSetting::default(),
            mixer: MixerSetting::default(),
            library_sort: LibrarySortSetting::default(),
            key_lock_on_new_tracks: true,
        }
    }
}

impl Settings {
    /// Where the settings live for this user.
    pub fn default_path() -> Result<PathBuf, SettingsError> {
        Ok(data_folder()?.settings())
    }

    /// Where the sets are kept.
    pub fn sets_path() -> Result<PathBuf, SettingsError> {
        Ok(data_folder()?.sets())
    }

    /// Where what Dubplate's analysis found in each track is kept between launches.
    pub fn analysis_cache_path() -> Result<PathBuf, SettingsError> {
        Ok(data_folder()?.analysis_cache())
    }

    /// Where separated stems and the separation model are kept.
    pub fn stems_location() -> Result<StemsLocation, SettingsError> {
        Ok(StemsLocation::in_data_folder(&data_folder()?))
    }

    /// The file that keeps a second Dubplate from starting beside this one.
    pub fn instance_lock_path() -> Result<PathBuf, SettingsError> {
        Ok(data_folder()?.instance_lock())
    }

    /// Where the scans of the music folders are kept between launches.
    pub fn library_cache_path() -> Result<PathBuf, SettingsError> {
        Ok(data_folder()?.library_cache())
    }

    /// Reads the settings at `path`. A missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(SettingsError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| {
            let backup = path.with_extension("toml.unreadable");
            if let Err(copy_error) = std::fs::copy(path, &backup) {
                return SettingsError::Io {
                    path: backup,
                    source: copy_error,
                };
            }
            SettingsError::Parse {
                path: path.to_path_buf(),
                backup,
                source: Box::new(source),
            }
        })
    }

    /// Writes the settings to `path`, through a temporary file so a crash never leaves half a
    /// file behind.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self)?;
        let io_error = |source| SettingsError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(io_error)?;
        }
        let temporary = path.with_extension("toml.partial");
        std::fs::write(&temporary, text).map_err(io_error)?;
        std::fs::rename(&temporary, path).map_err(io_error)
    }
}

/// Dubplate's own folder in the user's configuration, never one of rekordbox's.
fn data_folder() -> Result<DataFolder, SettingsError> {
    DataFolder::for_user().ok_or(SettingsError::NoConfigurationDirectory)
}

#[cfg(test)]
mod tests {
    use dubplate_library::Relink;

    use super::*;

    const FILE_NAME: &str = "settings.toml";

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join(FILE_NAME);
        let settings = Settings {
            lock_playing_deck: false,
            quantize: false,
            vinyl_mode: [true, false],
            audio_device: Some("Studio Speakers".to_owned()),
            library: LibrarySettings {
                music_folders: vec![
                    PathBuf::from("/Music/House"),
                    PathBuf::from("/Music/Amapiano"),
                ],
                import_rekordbox: true,
                relinks: vec![Relink {
                    from: PathBuf::from("/Music/Old/Track.m4a"),
                    to: PathBuf::from("/Music/New/Track.m4a"),
                }],
            },
            effect_units: vec![EffectUnitSetting {
                effect: "reverb".to_owned(),
                target: "M".to_owned(),
                parameter: "2".to_owned(),
                depth: 0.25,
                release: "vinyl brake".to_owned(),
            }],
            smart_cfx_preset: Some("dub echo".to_owned()),
            smart_cfx_on: true,
            sampler: SamplerSetting {
                volume: 0.6,
                slots: vec![SampleSlotSetting {
                    slot: 9,
                    path: PathBuf::from("/Music/Samples/Horn.wav"),
                    bpm: Some(120.0),
                    looped: false,
                    gate: true,
                    sync: false,
                }],
            },
            pad_fx: {
                let mut banks =
                    crate::pad_fx::PadFxBanks::from_preset(crate::pad_fx::PadFxPreset::Amapiano);
                banks.edit(crate::pad_fx::PadFxLayer::Fx2, 2, |slot| {
                    slot.toggle_trigger().step_parameter(1)
                });
                banks.setting()
            },
            screen: Some("prepare".to_owned()),
            levels: LevelsSetting {
                master: 0.4,
                headphones: 0.3,
                headphone_mix: 0.7,
            },
            auto_cue: AutoCueSetting {
                saved_cues_first: false,
                first_sound: true,
            },
            mixer: MixerSetting::of(
                &Crossfader {
                    enabled: false,
                    position: 0.5,
                    curve: CrossfaderCurve::Scratch,
                    assign: [CrossfaderAssign::Thru, CrossfaderAssign::A],
                },
                ChannelFaderCurve::Fast,
            ),
            library_sort: LibrarySortSetting {
                prepare: Some(SortSetting {
                    column: "bpm".to_owned(),
                    descending: true,
                }),
                perform: None,
            },
            key_lock_on_new_tracks: false,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert_eq!(
            LibrarySettings::load(&path).unwrap(),
            settings.library,
            "the library part reads back on its own, as the command line reads it"
        );
        assert!(!path.with_extension("toml.partial").exists());
    }

    #[test]
    fn a_file_with_every_setting_at_the_top_keeps_the_library() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "lock_playing_deck = false\nmusic_folders = [\"/Music/House\"]\n\
             import_rekordbox = true\n\n[[effect_units]]\neffect = \"echo\"\ntarget = \"M\"\n\
             beat = \"1\"\ndepth = 0.5\n\n[sampler]\nvolume = 0.6\nslots = []\n\n\
             [[relinks]]\nfrom = \"/Old/Track.m4a\"\nto = \"/New/Track.m4a\"\n",
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert!(!settings.lock_playing_deck);
        assert_eq!(
            settings.library.music_folders,
            [PathBuf::from("/Music/House")]
        );
        assert!(settings.library.import_rekordbox);
        assert_eq!(settings.library.relinks.len(), 1);
        assert_eq!(settings.effect_units.len(), 1);
        assert_eq!(settings.sampler.volume, 0.6);
    }

    #[test]
    fn mixer_switches_come_back_by_name_and_unknown_names_take_the_default() {
        let kept = MixerSetting::of(
            &Crossfader {
                curve: CrossfaderCurve::Constant,
                assign: [CrossfaderAssign::B, CrossfaderAssign::Thru],
                ..Crossfader::default()
            },
            ChannelFaderCurve::Slow,
        );
        assert!(kept.crossfader_on);
        assert_eq!(kept.crossfader_curve(), CrossfaderCurve::Constant);
        assert_eq!(kept.channel_fader_curve(), ChannelFaderCurve::Slow);
        assert_eq!(kept.crossfader_assign(DeckId::A), CrossfaderAssign::B);
        assert_eq!(kept.crossfader_assign(DeckId::B), CrossfaderAssign::Thru);
        let unknown = MixerSetting {
            crossfader_on: true,
            crossfader_curve: "wobbly".to_owned(),
            channel_fader_curve: String::new(),
            crossfader_assign: ["left".to_owned(), "right".to_owned()],
        };
        assert_eq!(unknown.crossfader_curve(), CrossfaderCurve::Blend);
        assert_eq!(unknown.channel_fader_curve(), ChannelFaderCurve::Standard);
        assert_eq!(unknown.crossfader_assign(DeckId::A), CrossfaderAssign::A);
        assert_eq!(unknown.crossfader_assign(DeckId::B), CrossfaderAssign::B);
    }

    #[test]
    fn key_lock_on_new_tracks_is_kept_off_once_turned_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        let mut settings = Settings::default();
        assert!(settings.key_lock_on_new_tracks);
        settings.key_lock_on_new_tracks = false;
        settings.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("key_lock_on_new_tracks = false"), "{saved}");
        assert!(!Settings::load(&path).unwrap().key_lock_on_new_tracks);
    }

    #[test]
    fn a_missing_file_or_setting_takes_the_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        std::fs::write(&path, "audio_device = \"Headphones\"\n").unwrap();
        let settings = Settings::load(&path).unwrap();
        assert!(settings.lock_playing_deck);
        assert!(
            settings.key_lock_on_new_tracks,
            "key lock is on for new tracks unless the DJ turned it off"
        );
        assert_eq!(settings.audio_device.as_deref(), Some("Headphones"));
        assert!(settings.library.music_folders.is_empty());
        assert!(
            !settings.library.import_rekordbox,
            "rekordbox is only read when the DJ asks"
        );
    }

    #[test]
    fn the_screen_and_levels_come_back_and_an_unknown_screen_opens_perform() {
        let mut settings = Settings::default();
        assert_eq!(settings.screen(), Mode::Perform);
        settings.set_screen(Mode::Prepare);
        assert_eq!(settings.screen(), Mode::Prepare);
        settings.screen = Some("edit".to_owned());
        assert_eq!(settings.screen(), Mode::Perform);

        let positions = LevelsSetting {
            master: 1.7,
            headphones: 0.25,
            headphone_mix: 0.6,
        }
        .apply(MixerPositions::default());
        assert_eq!(positions.master_level, 1.0);
        assert_eq!(LevelsSetting::of(&positions).headphones, 0.25);
        assert!(
            LevelsSetting::default().master < 1.0,
            "a first launch never opens the master at full"
        );
    }

    #[test]
    fn a_broken_file_says_which_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        std::fs::write(&path, "lock_playing_deck = \"sometimes\"\n").unwrap();
        let error = Settings::load(&path).unwrap_err();
        assert!(error.to_string().contains(FILE_NAME), "{error}");
        let backup = path.with_extension("toml.unreadable");
        assert_eq!(
            std::fs::read_to_string(backup).unwrap(),
            "lock_playing_deck = \"sometimes\"\n",
            "the unreadable file is kept before a save can replace it"
        );
    }

    #[test]
    fn settings_from_a_newer_version_keep_everything_this_version_knows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "audio_device = \"DDJ-FLX4\"\nmusic_folders = [\"/Music/Amapiano\"]\n\
             a_setting_from_the_future = true\n\n[another_future_table]\nlevel = 3\n",
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.audio_device.as_deref(), Some("DDJ-FLX4"));
        assert_eq!(
            settings.library.music_folders,
            vec![PathBuf::from("/Music/Amapiano")]
        );
    }
}
