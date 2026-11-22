//! Writes Device Libraries and reads them back with Dubplate's reader, checks the page layout
//! against the one rekordbox writes, and compares a small library with its golden description.

use std::path::{Path, PathBuf};

use dubplate_rekordbox::device_library::{
    Album, AlbumId, ArtistId, DatabaseFile, DeviceColor, DeviceLibrary, DeviceLibraryError,
    DeviceTrack, FileType, GenreId, KeyId, LabelId, LibraryId, Named, PageType, PlaylistEntry,
    PlaylistId, PlaylistNode, TableKind, TrackId, analysis_path, export_ext_pdb,
    parse_export_ext_tags,
};

fn id<Id>(make: fn(u32) -> Option<Id>, value: u32) -> Id {
    make(value).expect("test ids are not 0")
}

fn track(number: u32, title: &str, file_path: &str) -> DeviceTrack {
    DeviceTrack {
        id: id(TrackId::new, number),
        title: title.to_owned(),
        artist: None,
        composer: None,
        original_artist: None,
        remixer: None,
        album: None,
        genre: None,
        label: None,
        key: None,
        artwork: None,
        color: None,
        rating: 0,
        tempo_hundredths: 12_200,
        duration_seconds: 412,
        sample_rate: 44_100,
        sample_depth: 16,
        bitrate: 320,
        file_size: 9_876_543,
        file_type: FileType::Mp3,
        track_number: 0,
        disc_number: 0,
        year: 0,
        play_count: 0,
        checksum: number.wrapping_mul(0x9e37_79b9),
        file_path: file_path.to_owned(),
        analyze_path: analysis_path(file_path).to_string(),
        analyze_date: "2026-11-22".to_owned(),
        date_added: "2026-11-01".to_owned(),
        release_date: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        lyricist: String::new(),
        message: String::new(),
        kuvo_public: false,
        autoload_hot_cues: true,
    }
}

/// An afro house set: two tracks with every link filled, one with non-ASCII tags and a long
/// comment, in a playlist inside a folder.
fn small_library() -> DeviceLibrary {
    let mut library = DeviceLibrary::new(LibraryId(0x0dd0_b1a7), "2026-11-22");
    library.artists = vec![
        Named {
            id: id(ArtistId::new, 1),
            name: "Shimza".to_owned(),
        },
        Named {
            id: id(ArtistId::new, 2),
            name: "Mörda & Sébastien".to_owned(),
        },
    ];
    library.albums = vec![Album {
        id: id(AlbumId::new, 1),
        artist: Some(id(ArtistId::new, 1)),
        name: "Kabza Sessions".to_owned(),
    }];
    library.genres = vec![Named {
        id: id(GenreId::new, 1),
        name: "Afro House".to_owned(),
    }];
    library.labels = vec![Named {
        id: id(LabelId::new, 1),
        name: "Kunye".to_owned(),
    }];
    library.keys = vec![
        Named {
            id: id(KeyId::new, 1),
            name: "8A".to_owned(),
        },
        Named {
            id: id(KeyId::new, 2),
            name: "11B".to_owned(),
        },
    ];
    let mut first = track(
        1,
        "Akufanele",
        "/Contents/Shimza/Kabza Sessions/Akufanele.mp3",
    );
    first.artist = Some(id(ArtistId::new, 1));
    first.album = Some(id(AlbumId::new, 1));
    first.genre = Some(id(GenreId::new, 1));
    first.label = Some(id(LabelId::new, 1));
    first.key = Some(id(KeyId::new, 1));
    first.color = Some(DeviceColor::Aqua);
    first.rating = 4;
    first.year = 2025;
    first.track_number = 3;
    first.comment = "Mix in at the second drop".to_owned();
    let mut second = track(
        2,
        "Ngiyabonga (Mörda Remix)",
        "/Contents/Mörda & Sébastien/UnknownAlbum/Ngiyabonga.flac",
    );
    second.artist = Some(id(ArtistId::new, 2));
    second.remixer = Some(id(ArtistId::new, 2));
    second.key = Some(id(KeyId::new, 2));
    second.file_type = FileType::Flac;
    second.sample_depth = 24;
    second.bitrate = 2_116;
    second.tempo_hundredths = 11_850;
    second.comment = "ü".repeat(200);
    second.kuvo_public = true;
    library.tracks = vec![second, first];
    library.playlists = vec![
        PlaylistNode {
            id: id(PlaylistId::new, 1),
            parent: None,
            sort_order: 0,
            is_folder: true,
            name: "Sets".to_owned(),
        },
        PlaylistNode {
            id: id(PlaylistId::new, 2),
            parent: Some(id(PlaylistId::new, 1)),
            sort_order: 0,
            is_folder: false,
            name: "Saturday — Sunset".to_owned(),
        },
    ];
    library.playlist_entries = vec![
        PlaylistEntry {
            entry_index: 1,
            track: id(TrackId::new, 1),
            playlist: id(PlaylistId::new, 2),
        },
        PlaylistEntry {
            entry_index: 2,
            track: id(TrackId::new, 2),
            playlist: id(PlaylistId::new, 2),
        },
    ];
    library
}

/// The library as the reader gives it back: rows in id order and no creation date, which the
/// file does not keep.
fn normalized(mut library: DeviceLibrary) -> DeviceLibrary {
    library.created = String::new();
    library.tracks.sort_by_key(|track| track.id);
    library.artists.sort_by_key(|artist| artist.id);
    library.albums.sort_by_key(|album| album.id);
    library.genres.sort_by_key(|genre| genre.id);
    library.labels.sort_by_key(|label| label.id);
    library.keys.sort_by_key(|key| key.id);
    library.artwork.sort_by_key(|artwork| artwork.id);
    library.playlists.sort_by_key(|node| node.id);
    library
        .playlist_entries
        .sort_by_key(|entry| (entry.playlist, entry.entry_index));
    library.preserved = Default::default();
    library
}

#[test]
fn every_field_reads_back() {
    let library = small_library();
    let bytes = library.to_export_pdb().unwrap();
    let read = DeviceLibrary::parse_export_pdb(&bytes).unwrap();
    assert_eq!(normalized(read.clone()), normalized(library));
    assert_eq!(
        read.preserved.rows(TableKind::HistorySync).len(),
        1,
        "the history sync row is kept as read"
    );
    assert!(!read.preserved.has_history());
}

#[test]
fn the_layout_is_the_one_rekordbox_writes() {
    let bytes = small_library().to_export_pdb().unwrap();
    assert_eq!(bytes.len() % 4096, 0);
    let database = DatabaseFile::parse(&bytes).unwrap();
    assert_eq!(database.page_size, 4096);
    assert_eq!(database.header_unknown, 5);
    assert_eq!(database.tables.len(), 20);
    for (position, table) in database.tables.iter().enumerate() {
        let position = position as u32;
        assert_eq!(table.type_number, position);
        assert_eq!(table.first_page.0, 2 * position + 1, "table {position}");
        assert_eq!(table.pages[0].page_type, PageType::Index);
        assert_eq!(table.pages[0].flags, 0x64);
        let last = table.pages.last().unwrap();
        assert_eq!(last.index, table.last_page);
        assert_eq!(last.next_page, table.empty_candidate, "table {position}");
        if table.pages.len() > 1 {
            assert_eq!(table.pages[1].index.0, 2 * position + 2);
            assert!(table.pages[1..].iter().all(|page| page.flags == 0x24));
        } else {
            assert_eq!(table.empty_candidate.0, 2 * position + 2);
        }
        assert!(table.empty_candidate < database.next_unused_page);
    }
    let rows = |kind: TableKind| database.table(kind.type_number()).unwrap().rows.len();
    assert_eq!(rows(TableKind::Colors), 8);
    assert_eq!(rows(TableKind::Columns), 27);
    assert_eq!(rows(TableKind::Unknown17), 22);
    assert_eq!(rows(TableKind::Unknown18), 17);
    assert_eq!(rows(TableKind::HistorySync), 1);
    assert_eq!(rows(TableKind::Tracks), 2);
    assert_eq!(rows(TableKind::HistoryPlaylists), 0);
}

#[test]
fn a_big_library_fills_page_after_page() {
    let mut library = DeviceLibrary::new(LibraryId(1), "2026-11-22");
    library.playlists.push(PlaylistNode {
        id: id(PlaylistId::new, 1),
        parent: None,
        sort_order: 0,
        is_folder: false,
        name: "Everything".to_owned(),
    });
    for number in 1..=1_500 {
        library.tracks.push(track(
            number,
            &format!("Track {number}"),
            &format!("/Contents/UnknownArtist/UnknownAlbum/Track {number}.mp3"),
        ));
        library.playlist_entries.push(PlaylistEntry {
            entry_index: number,
            track: id(TrackId::new, number),
            playlist: id(PlaylistId::new, 1),
        });
    }
    let bytes = library.to_export_pdb().unwrap();
    let database = DatabaseFile::parse(&bytes).unwrap();
    let tracks = database.table(0).unwrap();
    assert!(tracks.pages.len() > 100);
    let read = DeviceLibrary::parse_export_pdb(&bytes).unwrap();
    assert_eq!(read.tracks.len(), 1_500);
    assert_eq!(read.playlist_entries.len(), 1_500);
    assert_eq!(normalized(read), normalized(library));
}

#[test]
fn a_link_to_a_missing_row_is_refused() {
    let mut library = small_library();
    library.tracks[0].genre = GenreId::new(42);
    assert!(matches!(
        library.to_export_pdb(),
        Err(DeviceLibraryError::DanglingReference(_))
    ));
    let mut library = small_library();
    library.playlist_entries[0].playlist = id(PlaylistId::new, 1);
    assert!(matches!(
        library.to_export_pdb(),
        Err(DeviceLibraryError::DanglingReference(_))
    ));
    let mut library = small_library();
    library.artists.push(library.artists[0].clone());
    assert!(matches!(
        library.to_export_pdb(),
        Err(DeviceLibraryError::DanglingReference(_))
    ));
}

#[test]
fn history_players_record_is_written_back_as_read() {
    let library = small_library();
    let bytes = library.to_export_pdb().unwrap();
    let mut read = DeviceLibrary::parse_export_pdb(&bytes).unwrap();
    read.tracks.retain(|track| track.id.get() == 1);
    read.playlist_entries.retain(|entry| entry.track.get() == 1);
    let rewritten = DeviceLibrary::parse_export_pdb(&read.to_export_pdb().unwrap()).unwrap();
    assert_eq!(rewritten.preserved, read.preserved);
    assert_eq!(rewritten.tracks.len(), 1);
}

#[test]
fn the_ext_database_carries_rekordbox_s_default_tags() {
    let bytes = export_ext_pdb(LibraryId(9)).unwrap();
    let database = DatabaseFile::parse(&bytes).unwrap();
    assert_eq!(database.tables.len(), 9);
    assert_eq!(database.table(7).unwrap().rows.len(), 1);
    let tags = parse_export_ext_tags(&bytes).unwrap();
    let categories: Vec<&str> = tags
        .iter()
        .filter(|tag| tag.is_category)
        .map(|tag| tag.name.as_str())
        .collect();
    assert_eq!(
        categories,
        ["Genre", "Components", "Situation", "Untitled Column"]
    );
}

fn describe(bytes: &[u8]) -> String {
    let database = DatabaseFile::parse(bytes).expect("the file reads");
    let mut text = format!(
        "{} bytes, page size {}, next unused page {}, header 0x10 {}, sequence {}\n",
        bytes.len(),
        database.page_size,
        database.next_unused_page,
        database.header_unknown,
        database.sequence
    );
    for table in &database.tables {
        text.push_str(&format!(
            "table {}: first {} last {} empty {}\n",
            table.type_number, table.first_page, table.last_page, table.empty_candidate
        ));
        for page in &table.pages {
            text.push_str(&format!(
                "  page {} {:?} next {} sequence {} flags {:#04x} slots {} present {} free {} used {}\n",
                page.index,
                page.page_type,
                page.next_page,
                page.sequence,
                page.flags,
                page.slot_count,
                page.present_count,
                page.free_size,
                page.used_size
            ));
        }
        for row in &table.rows {
            let hex: String = row.iter().map(|byte| format!("{byte:02x}")).collect();
            text.push_str(&format!("  row {hex}\n"));
        }
    }
    text
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("device_library")
        .join(name)
}

/// Compares `actual` with the golden file `name`; `DUBPLATE_BLESS=1` writes it instead.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("DUBPLATE_BLESS").is_some() {
        let folder = path.parent().expect("a golden file sits in a folder");
        std::fs::create_dir_all(folder).expect("the golden folder can be made");
        std::fs::write(&path, actual).expect("the golden file can be written");
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}; run with DUBPLATE_BLESS=1", path.display()));
    assert!(
        expected == actual,
        "{} changed; review the difference and run with DUBPLATE_BLESS=1 to accept it",
        path.display()
    );
}

/// Writes the golden library's files into the folder `DUBPLATE_DEVICE_LIBRARY_SAMPLES` names,
/// as a stick's `PIONEER/rekordbox`, for independent readers to check.
fn write_samples(export: &[u8], export_ext: &[u8]) {
    let Some(folder) = std::env::var_os("DUBPLATE_DEVICE_LIBRARY_SAMPLES") else {
        return;
    };
    let folder = Path::new(&folder).join("PIONEER").join("rekordbox");
    std::fs::create_dir_all(&folder).expect("the sample folder can be made");
    std::fs::write(folder.join("export.pdb"), export).expect("the sample can be written");
    std::fs::write(folder.join("exportExt.pdb"), export_ext).expect("the sample can be written");
}

#[test]
fn a_small_library_matches_its_golden_file() {
    let export = small_library().to_export_pdb().unwrap();
    let export_ext = export_ext_pdb(LibraryId(0x0dd0_b1a7)).unwrap();
    write_samples(&export, &export_ext);
    assert_golden("small-export.pdb.txt", &describe(&export));
    assert_golden("export-ext.pdb.txt", &describe(&export_ext));
}

/// Reads real rekordbox exports from the folder `DUBPLATE_REKORDBOX_EXPORTS` names, each a
/// stick's root with `PIONEER/rekordbox/export.pdb`, checks every analysis path against the
/// hash, and writes each library back and reads it again.
#[test]
#[ignore = "needs real rekordbox exports; set DUBPLATE_REKORDBOX_EXPORTS"]
fn reads_real_rekordbox_exports() {
    let Some(root) = std::env::var_os("DUBPLATE_REKORDBOX_EXPORTS") else {
        return;
    };
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let stick = entry.unwrap().path();
        let pdb = ["PIONEER", ".PIONEER"]
            .into_iter()
            .map(|folder| stick.join(folder).join("rekordbox").join("export.pdb"))
            .find(|path| path.is_file());
        let Some(pdb) = pdb else { continue };
        let bytes = std::fs::read(&pdb).unwrap();
        let library = DeviceLibrary::parse_export_pdb(&bytes)
            .unwrap_or_else(|error| panic!("{}: {error}", pdb.display()));
        for track in &library.tracks {
            let expected = analysis_path(&track.file_path).to_string();
            assert_eq!(
                track.analyze_path.replacen("/.PIONEER/", "/PIONEER/", 1),
                expected,
                "{}",
                pdb.display()
            );
        }
        let rewritten = DeviceLibrary::parse_export_pdb(&library.to_export_pdb().unwrap()).unwrap();
        assert_eq!(normalized(rewritten), normalized(library.clone()));
        checked += library.tracks.len();
    }
    assert!(checked > 0, "no export.pdb found");
}
