//! Scanning a music folder of 700 tagged files, built in a temporary folder so the bench never
//! reads the DJ's own music. The scan reads tags and lengths only, never the audio.

use std::hint::black_box;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

use criterion::{Criterion, criterion_group, criterion_main};
use dubplate_library::scan_folder;
use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};

const FILE_COUNT: usize = 700;
const FILES_PER_FOLDER: usize = 25;

/// A tenth of a second of silent 16-bit stereo WAV.
fn wav_bytes() -> Vec<u8> {
    let sample_rate: u32 = 44_100;
    let data_length = sample_rate / 10 * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    bytes.resize(bytes.len() + data_length as usize, 0);
    bytes
}

/// 700 tagged WAV files in folders of 25, by artist, with a cover and a text file in each
/// folder that the scan skips.
fn build_music_folder(root: &Path) {
    let wav = wav_bytes();
    for index in 0..FILE_COUNT {
        let folder = root.join(format!("Artist {:02}", index / FILES_PER_FOLDER));
        std::fs::create_dir_all(&folder).expect("a folder");
        if index % FILES_PER_FOLDER == 0 {
            std::fs::write(folder.join("cover.jpg"), b"not audio").expect("a cover");
            std::fs::write(folder.join("notes.txt"), b"not audio").expect("notes");
        }
        let path = folder.join(format!("Track {index:03}.wav"));
        std::fs::write(&path, &wav).expect("a WAV file");
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Track {index:03}"));
        tag.set_artist(format!(
            "Artist {:02}; Guest {}",
            index / FILES_PER_FOLDER,
            index % 7
        ));
        tag.set_album("Bench Crate".to_owned());
        tag.insert_text(ItemKey::IntegerBpm, format!("{}", 118 + index % 14));
        tag.insert_text(ItemKey::InitialKey, format!("{}A", index % 12 + 1));
        tag.save_to_path(&path, WriteOptions::default())
            .expect("tag the WAV file");
    }
}

fn scan(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("scan_700_files");
    group.sample_size(20);
    let music = tempfile::tempdir().expect("a temporary folder");
    build_music_folder(music.path());
    let progress = AtomicUsize::new(0);
    group.bench_function("first_scan_reads_every_tag", |bencher| {
        bencher.iter(|| scan_folder(black_box(music.path()), None, &progress).expect("scan"));
    });
    let previous = scan_folder(music.path(), None, &progress).expect("scan");
    assert_eq!(previous.tracks.len(), FILE_COUNT);
    group.bench_function("rescan_with_nothing_changed", |bencher| {
        bencher.iter(|| {
            scan_folder(black_box(music.path()), Some(&previous), &progress).expect("rescan")
        });
    });
    group.finish();
}

criterion_group!(benches, scan);
criterion_main!(benches);
