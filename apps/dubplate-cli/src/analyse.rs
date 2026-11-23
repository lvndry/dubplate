//! `dubplate analyse`: Dubplate's own analysis of audio files (tempo, beat grid, key, energy
//! and loudness), optionally compared with rekordbox's for the tracks it also holds, including
//! the loudness rekordbox's Auto Gain levels them to.

use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;

use anyhow::{Context, Result, bail};
use dubplate_library::{is_audio_file, scan_folder};
use dubplate_prep::{AudioAnalysis, Key, analyse_track};
use dubplate_rekordbox::{DatabaseKey, Library, Track};

/// Two tempos this close, in BPM, agree.
const SAME_TEMPO_BPM: f32 = 0.5;
/// A rekordbox downbeat this close to one of the analysed grid's beats, in milliseconds, sits
/// on it.
const ON_THE_BEAT_MS: f64 = 20.0;
const BEATS_PER_BAR: i64 = 4;

/// What `dubplate analyse` is asked to do.
#[derive(clap::Args)]
pub(crate) struct AnalyseArgs {
    /// Audio files or folders to analyse. Folders are searched with their subfolders.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Compare with rekordbox's BPM, key and beat grid for the tracks it also holds, found by
    /// their file or, failing that, their file name. rekordbox's files are only read.
    #[arg(long)]
    compare_rekordbox: bool,
    /// The library's master.db, for --compare-rekordbox. Defaults to rekordbox's usual
    /// location for this user.
    #[arg(long)]
    library: Option<PathBuf>,
    /// How many tracks are analysed at once. Defaults to the number of processor cores.
    #[arg(long)]
    jobs: Option<NonZeroUsize>,
}

/// One analysed file.
struct Analysed {
    path: PathBuf,
    outcome: Result<AudioAnalysis, String>,
}

/// What rekordbox holds for a track that was also analysed.
struct RekordboxReading {
    bpm: Option<f32>,
    key: Option<Key>,
    first_downbeat_ms: Option<u32>,
    auto_gain_db: Option<f32>,
}

pub(crate) fn run(arguments: AnalyseArgs) -> Result<()> {
    let files = audio_files(&arguments.paths)?;
    if files.is_empty() {
        bail!("no audio files found");
    }
    let rekordbox = if arguments.compare_rekordbox {
        Some(rekordbox_readings(crate::master_db_or_default(
            arguments.library,
        )?)?)
    } else {
        None
    };
    let jobs = arguments
        .jobs
        .or_else(|| std::thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get);
    let mut analysed = analyse_all(&files, jobs);
    analysed.sort_by(|first, second| first.path.cmp(&second.path));

    let mut comparison = Comparison::default();
    println!(
        "{:>7} {:>4} {:>8} {:>3} {:>6}  {:>7} {:>4} {:>6} {:>6}  File",
        "BPM", "Key", "Downbeat", "En", "LUFS", "rb BPM", "rb", "Grid", "rb AG"
    );
    for track in &analysed {
        let reading = rekordbox
            .as_ref()
            .and_then(|rekordbox| rekordbox.find(&track.path));
        print_track(track, reading);
        if let (Ok(analysis), Some(reading)) = (&track.outcome, reading) {
            comparison.add(analysis, reading);
        }
    }
    println!();
    print_summary(&analysed, &arguments.paths);
    if rekordbox.is_some() {
        comparison.print();
    }
    Ok(())
}

/// Every audio file in `paths`, folders searched with their subfolders.
fn audio_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let scan = scan_folder(path, None, &AtomicUsize::new(0))
                .with_context(|| format!("searching {}", path.display()))?;
            files.extend(scan.tracks.into_iter().map(|track| track.path));
        } else if is_audio_file(path) {
            files.push(path.clone());
        } else {
            bail!("{} is neither a folder nor an audio file", path.display());
        }
    }
    Ok(files)
}

/// Analyses every file on `jobs` threads, telling how far it has come on standard error.
fn analyse_all(files: &[PathBuf], jobs: usize) -> Vec<Analysed> {
    let next = AtomicUsize::new(0);
    let (sender, receiver) = channel();
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(files.len()) {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                while let Some(path) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = analyse_track(path).map_err(|error| error.to_string());
                    let analysed = Analysed {
                        path: path.clone(),
                        outcome,
                    };
                    if sender.send(analysed).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut analysed = Vec::with_capacity(files.len());
        for track in receiver {
            analysed.push(track);
            eprint!("\ranalysed {} of {}", analysed.len(), files.len());
        }
        eprintln!();
        analysed
    })
}

/// rekordbox's tracks, found by file and by file name.
struct RekordboxReadings {
    by_path: HashMap<PathBuf, RekordboxReading>,
    by_name: HashMap<String, Option<PathBuf>>,
}

impl RekordboxReadings {
    /// The rekordbox track whose file is `path`, or the only one with the same file name.
    fn find(&self, path: &Path) -> Option<&RekordboxReading> {
        if let Some(reading) = self.by_path.get(&canonical(path)) {
            return Some(reading);
        }
        let unique = self.by_name.get(&file_name(path)?)?.as_ref()?;
        self.by_path.get(unique)
    }
}

/// Reads a copy of the library at `master_db` and each track's BPM, key and first downbeat.
fn rekordbox_readings(master_db: PathBuf) -> Result<RekordboxReadings> {
    let key = DatabaseKey::from_user_configuration()?;
    let library = Library::open_copy(&master_db, &key)
        .with_context(|| format!("reading {}", master_db.display()))?;
    let mut readings = RekordboxReadings {
        by_path: HashMap::new(),
        by_name: HashMap::new(),
    };
    for track in library.tracks()? {
        let Some(path) = track.file_path.as_deref().map(canonical) else {
            continue;
        };
        if let Some(name) = file_name(&path) {
            readings
                .by_name
                .entry(name)
                .and_modify(|unique| *unique = None)
                .or_insert_with(|| Some(path.clone()));
        }
        readings
            .by_path
            .insert(path, rekordbox_reading(&library, &track));
    }
    Ok(readings)
}

fn rekordbox_reading(library: &Library, track: &Track) -> RekordboxReading {
    let first_downbeat_ms = library
        .analysis(track)
        .ok()
        .and_then(|analysis| analysis.beat_grid)
        .and_then(|grid| grid.first_downbeat().map(|beat| beat.time_ms));
    RekordboxReading {
        bpm: track.bpm.map(|bpm| bpm as f32),
        key: track.key.as_deref().and_then(Key::parse_file_key),
        first_downbeat_ms,
        auto_gain_db: track.auto_gain_db,
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn file_name(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_string_lossy().to_lowercase())
}

fn print_track(track: &Analysed, reading: Option<&RekordboxReading>) {
    let name = track.path.display();
    let analysis = match &track.outcome {
        Ok(analysis) => analysis,
        Err(error) => {
            println!(
                "{:>7} {:>4} {:>8} {:>3} {:>6}  {:>33}  {name}: {error}",
                "", "", "", "", "", "failed"
            );
            return;
        }
    };
    let bpm = analysis
        .bpm
        .map(|bpm| format!("{bpm:.2}"))
        .unwrap_or_default();
    let key = analysis.key.map(|key| key.camelot()).unwrap_or_default();
    let downbeat = analysis
        .beat_grid
        .map(|grid| format!("{:.3}s", grid.first_downbeat_seconds))
        .unwrap_or_default();
    let energy = analysis
        .energy
        .as_ref()
        .map(|energy| energy.rating.to_string())
        .unwrap_or_default();
    let loudness = analysis
        .loudness
        .map(|loudness| format!("{:.1}", loudness.0))
        .unwrap_or_default();
    let (rekordbox_bpm, rekordbox_key, grid, auto_gain) = match reading {
        Some(reading) => (
            reading
                .bpm
                .map(|bpm| format!("{bpm:.2}"))
                .unwrap_or_default(),
            reading.key.map(|key| key.camelot()).unwrap_or_default(),
            grid_agreement(analysis, reading)
                .map(|agreement| agreement.describe())
                .unwrap_or_default(),
            reading
                .auto_gain_db
                .map(|gain| format!("{gain:+.1}"))
                .unwrap_or_default(),
        ),
        None => Default::default(),
    };
    println!(
        "{bpm:>7} {key:>4} {downbeat:>8} {energy:>3} {loudness:>6}  {rekordbox_bpm:>7} \
         {rekordbox_key:>4} {grid:>6} {auto_gain:>6}  {name}"
    );
}

/// How the analysed grid lines up with rekordbox's.
#[derive(Debug, Clone, Copy)]
struct GridAgreement {
    /// How far rekordbox's first downbeat is from the analysed grid's nearest beat, later when
    /// positive.
    offset_ms: f64,
    /// Whether that beat is a downbeat of the analysed grid.
    same_downbeat: bool,
    /// The length of a beat of the analysed grid.
    beat_ms: f64,
}

impl GridAgreement {
    fn on_the_beat(self) -> bool {
        self.offset_ms.abs() <= ON_THE_BEAT_MS
    }

    /// Whether rekordbox's beats fall closer to the analysed grid's off-beats than to its
    /// beats: the grid is half a beat off.
    fn on_the_off_beat(self) -> bool {
        self.offset_ms.abs() > self.beat_ms / 4.0
    }

    /// Whether the analysed grid's bars start where rekordbox's do: its beats on rekordbox's
    /// and its downbeat on rekordbox's downbeat.
    fn same_bars(self) -> bool {
        self.same_downbeat && !self.on_the_off_beat()
    }

    fn describe(self) -> String {
        let bar = if self.same_downbeat { "" } else { "~" };
        format!("{bar}{:+.0}ms", self.offset_ms)
    }
}

/// The grids' agreement, when both have one at the same tempo.
fn grid_agreement(analysis: &AudioAnalysis, reading: &RekordboxReading) -> Option<GridAgreement> {
    let grid = analysis.beat_grid?;
    let same_tempo = (analysis.bpm? - reading.bpm?).abs() <= SAME_TEMPO_BPM;
    if !same_tempo {
        return None;
    }
    let rekordbox_seconds = f64::from(reading.first_downbeat_ms?) / 1000.0;
    let beats = ((rekordbox_seconds - grid.first_downbeat_seconds) / grid.beat_seconds()).round();
    let nearest_seconds = grid.first_downbeat_seconds + beats * grid.beat_seconds();
    Some(GridAgreement {
        offset_ms: (rekordbox_seconds - nearest_seconds) * 1000.0,
        same_downbeat: (beats as i64).rem_euclid(BEATS_PER_BAR) == 0,
        beat_ms: grid.beat_seconds() * 1000.0,
    })
}

/// How each track's tempo, key and grid compare with rekordbox's.
#[derive(Default)]
struct Comparison {
    matched: usize,
    tempo_compared: usize,
    tempo_agrees: usize,
    tempo_octave: Vec<(f32, f32)>,
    tempo_other: Vec<(f32, f32)>,
    key_compared: usize,
    key_agrees: usize,
    key_neighbour: usize,
    grid_compared: usize,
    grid_on_the_beat: usize,
    grid_on_the_off_beat: usize,
    grid_same_bars: usize,
    grid_offsets_ms: Vec<f64>,
    auto_gain_levels: Vec<f32>,
}

impl Comparison {
    fn add(&mut self, analysis: &AudioAnalysis, reading: &RekordboxReading) {
        self.matched += 1;
        if let (Some(analysed), Some(rekordbox)) = (analysis.bpm, reading.bpm) {
            self.tempo_compared += 1;
            let octave = [analysed * 2.0, analysed / 2.0]
                .iter()
                .any(|moved| (moved - rekordbox).abs() <= SAME_TEMPO_BPM);
            if (analysed - rekordbox).abs() <= SAME_TEMPO_BPM {
                self.tempo_agrees += 1;
            } else if octave {
                self.tempo_octave.push((analysed, rekordbox));
            } else {
                self.tempo_other.push((analysed, rekordbox));
            }
        }
        if let (Some(analysed), Some(rekordbox)) = (analysis.key, reading.key) {
            self.key_compared += 1;
            if analysed == rekordbox {
                self.key_agrees += 1;
            } else if neighbours(analysed, rekordbox) {
                self.key_neighbour += 1;
            }
        }
        if let Some(agreement) = grid_agreement(analysis, reading) {
            self.grid_compared += 1;
            self.grid_offsets_ms.push(agreement.offset_ms);
            self.grid_on_the_beat += usize::from(agreement.on_the_beat());
            self.grid_on_the_off_beat += usize::from(agreement.on_the_off_beat());
            self.grid_same_bars += usize::from(agreement.same_bars());
        }
        if let (Some(loudness), Some(gain)) = (analysis.loudness, reading.auto_gain_db) {
            self.auto_gain_levels.push(loudness.0 + gain);
        }
    }

    fn print(&mut self) {
        println!("Compared with rekordbox: {} tracks in both", self.matched);
        println!(
            "  BPM within {SAME_TEMPO_BPM}: {}",
            share(self.tempo_agrees, self.tempo_compared)
        );
        println!(
            "  BPM half or double rekordbox's: {}",
            share(self.tempo_octave.len(), self.tempo_compared)
        );
        println!(
            "  BPM otherwise different: {}",
            share(self.tempo_other.len(), self.tempo_compared)
        );
        for (label, misses) in [("octave", &self.tempo_octave), ("other", &self.tempo_other)] {
            for (analysed, rekordbox) in misses {
                println!("    {label}: analysed {analysed:.2}, rekordbox {rekordbox:.2}");
            }
        }
        println!(
            "  Key exactly rekordbox's: {}",
            share(self.key_agrees, self.key_compared)
        );
        println!(
            "  Key one step or relative to rekordbox's: {}",
            share(self.key_neighbour, self.key_compared)
        );
        self.grid_offsets_ms.sort_by(f64::total_cmp);
        let median = self
            .grid_offsets_ms
            .get(self.grid_offsets_ms.len() / 2)
            .copied()
            .unwrap_or_default();
        println!(
            "  Grid on rekordbox's beats within {ON_THE_BEAT_MS} ms: {} (median offset {median:+.1} ms)",
            share(self.grid_on_the_beat, self.grid_compared)
        );
        println!(
            "  Grid half a beat off rekordbox's: {}",
            share(self.grid_on_the_off_beat, self.grid_compared)
        );
        println!(
            "  Same downbeat as rekordbox: {}",
            share(self.grid_same_bars, self.grid_compared)
        );
        self.auto_gain_levels.sort_by(f32::total_cmp);
        let percentile = |fraction: f32| {
            let last = self.auto_gain_levels.len().checked_sub(1)?;
            let index = (last as f32 * fraction).round() as usize;
            self.auto_gain_levels.get(index).copied()
        };
        match (percentile(0.1), percentile(0.5), percentile(0.9)) {
            (Some(low), Some(median), Some(high)) => println!(
                "  rekordbox's Auto Gain levels {} tracks to {median:.1} LUFS as Dubplate \
                 measures it (80% between {low:.1} and {high:.1})",
                self.auto_gain_levels.len()
            ),
            _ => println!("  rekordbox's Auto Gain: none to compare"),
        }
    }
}

/// Whether two keys are one step apart on the Camelot wheel or relative major and minor.
fn neighbours(first: Key, second: Key) -> bool {
    let steps =
        (i16::from(first.camelot_number()) - i16::from(second.camelot_number())).rem_euclid(12);
    let same_mode = first.mode() == second.mode();
    (same_mode && (steps == 1 || steps == 11)) || (!same_mode && steps == 0)
}

fn share(count: usize, total: usize) -> String {
    if total == 0 {
        return "none to compare".to_owned();
    }
    format!(
        "{count} of {total} ({:.1}%)",
        count as f64 / total as f64 * 100.0
    )
}

/// How many tracks were analysed, and the tempos found in each folder under `roots`.
fn print_summary(analysed: &[Analysed], roots: &[PathBuf]) {
    let failed = analysed
        .iter()
        .filter(|track| track.outcome.is_err())
        .count();
    let without_tempo = analysed
        .iter()
        .filter(|track| matches!(&track.outcome, Ok(analysis) if analysis.bpm.is_none()))
        .count();
    let without_key = analysed
        .iter()
        .filter(|track| matches!(&track.outcome, Ok(analysis) if analysis.key.is_none()))
        .count();
    println!(
        "Analysed {} files: {failed} failed, {without_tempo} without a tempo, {without_key} without a key",
        analysed.len()
    );
    let mut by_folder: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for track in analysed {
        if let Ok(analysis) = &track.outcome {
            by_folder
                .entry(top_folder(&track.path, roots))
                .or_default()
                .extend(analysis.bpm);
        }
    }
    for (folder, mut tempos) in by_folder {
        if tempos.is_empty() {
            continue;
        }
        tempos.sort_by(f32::total_cmp);
        println!(
            "  {folder}: {} tracks, BPM {:.1} to {:.1}, median {:.1}",
            tempos.len(),
            tempos[0],
            tempos[tempos.len() - 1],
            tempos[tempos.len() / 2]
        );
    }
}

/// The folder a track is counted in: the first folder under the folder it was found in, such
/// as a genre, or that folder itself for a track directly in it.
fn top_folder(path: &Path, roots: &[PathBuf]) -> String {
    let name = |folder: &Path| {
        folder
            .file_name()
            .unwrap_or(folder.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    let inside = roots
        .iter()
        .find_map(|root| Some((root, path.strip_prefix(root).ok()?)));
    match inside {
        Some((root, relative)) => match relative.parent().and_then(|parent| parent.iter().next()) {
            Some(first) => first.to_string_lossy().into_owned(),
            None => name(root),
        },
        None => path.parent().map(name).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(camelot: &str) -> Key {
        Key::parse_file_key(camelot).unwrap()
    }

    #[test]
    fn a_track_is_counted_in_the_first_folder_under_where_it_was_found() {
        let roots = [PathBuf::from("/Music/DJ tracks")];
        let folder = |path: &str| top_folder(Path::new(path), &roots);
        assert_eq!(
            folder("/Music/DJ tracks/amapiano/build/One.m4a"),
            "amapiano"
        );
        assert_eq!(folder("/Music/DJ tracks/Loose.mp3"), "DJ tracks");
        assert_eq!(folder("/Elsewhere/Set/Other.mp3"), "Set");
    }

    #[test]
    fn neighbours_are_one_step_on_the_wheel_or_relative() {
        assert!(neighbours(key("8A"), key("9A")));
        assert!(neighbours(key("12A"), key("1A")));
        assert!(neighbours(key("8A"), key("8B")));
        assert!(!neighbours(key("8A"), key("9B")));
        assert!(!neighbours(key("8A"), key("8A")));
    }

    #[test]
    fn a_rekordbox_downbeat_is_placed_on_the_analysed_grid() {
        let analysis = AudioAnalysis {
            duration_seconds: 60.0,
            bpm: Some(120.0),
            beat_grid: Some(dubplate_prep::AnalysedGrid {
                bpm: 120.0,
                first_downbeat_seconds: 0.5,
            }),
            key: None,
            energy: None,
            loudness: None,
        };
        let reading = |first_downbeat_ms| RekordboxReading {
            bpm: Some(120.0),
            key: None,
            first_downbeat_ms: Some(first_downbeat_ms),
            auto_gain_db: None,
        };
        let same = grid_agreement(&analysis, &reading(2_510)).unwrap();
        assert!((same.offset_ms - 10.0).abs() < 1e-6);
        assert!(same.same_bars() && same.on_the_beat());
        let shifted = grid_agreement(&analysis, &reading(1_000)).unwrap();
        assert!(!shifted.same_downbeat && !shifted.same_bars());
        let off_beat = grid_agreement(&analysis, &reading(2_750)).unwrap();
        assert!(off_beat.on_the_off_beat() && !off_beat.same_bars());
        let other_tempo = RekordboxReading {
            bpm: Some(122.0),
            ..reading(500)
        };
        assert!(grid_agreement(&analysis, &other_tempo).is_none());
    }
}
