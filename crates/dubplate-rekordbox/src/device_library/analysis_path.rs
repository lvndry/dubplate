//! Where rekordbox puts a track's analysis files on a stick.
//!
//! The folder comes from a hash of the audio file's path on the stick, as UTF-16 code units:
//! `/PIONEER/USBANLZ/P{folder:03X}/{hash:08X}/ANLZ0000.DAT`. Players work the folder out the
//! same way, so it has to match. The function reproduces every `analyze_path` of the real
//! rekordbox 6 and 7 exports it was checked against (21 tracks in four exports).

use std::fmt;

use crate::analysis::AnalysisFileKind;

const MULTIPLIER: u32 = 0x5bc9;
const SECOND_MULTIPLIER: u32 = 0x93b5;
const MODULUS: u32 = 200_003;
/// The bits of the hash that make up the folder number, lowest first.
const FOLDER_BITS: [u32; 7] = [0, 2, 6, 7, 9, 13, 16];
const ROOT: &str = "/PIONEER/USBANLZ";

/// The analysis files of one track on a stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisPath {
    hash: u32,
    number: u16,
}

/// The analysis files rekordbox gives the audio file at `device_path`, the path players see,
/// such as `/Contents/Artist/Album/Track.mp3`.
pub fn analysis_path(device_path: &str) -> AnalysisPath {
    let hash = device_path.encode_utf16().fold(0_u32, |hash, unit| {
        let unit = u32::from(unit);
        hash.wrapping_mul(MULTIPLIER)
            .wrapping_add(unit)
            .wrapping_mul(SECOND_MULTIPLIER)
            .wrapping_add(unit)
    }) % MODULUS;
    AnalysisPath { hash, number: 0 }
}

impl AnalysisPath {
    /// The files numbered `number` in the same folder, which rekordbox uses when two tracks'
    /// paths hash to one folder: the second gets `ANLZ0001`.
    #[must_use]
    pub fn numbered(self, number: u16) -> Self {
        Self { number, ..self }
    }

    /// The number in the file names, 0 for `ANLZ0000`.
    pub fn number(self) -> u16 {
        self.number
    }

    fn folder_number(self) -> u32 {
        FOLDER_BITS
            .iter()
            .enumerate()
            .fold(0, |folder, (position, bit)| {
                folder | ((self.hash >> bit) & 1) << position
            })
    }

    /// The folder, from the root of the stick: `/PIONEER/USBANLZ/P016/0000875E`.
    pub fn folder(self) -> String {
        format!("{ROOT}/P{:03X}/{:08X}", self.folder_number(), self.hash)
    }

    /// One of the files, from the root of the stick: `…/ANLZ0000.DAT`.
    pub fn file(self, kind: AnalysisFileKind) -> String {
        format!(
            "{}/ANLZ{:04X}.{}",
            self.folder(),
            self.number,
            kind.extension()
        )
    }
}

impl fmt::Display for AnalysisPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.file(AnalysisFileKind::Dat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_folders_of_real_rekordbox_exports() {
        for (path, expected) in [
            (
                "/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3",
                "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT",
            ),
            (
                "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3",
                "/PIONEER/USBANLZ/P053/0001D21F/ANLZ0000.DAT",
            ),
            (
                "/Contents/UnknownArtist/UnknownAlbum/SIREN.wav",
                "/PIONEER/USBANLZ/P017/00009B77/ANLZ0000.DAT",
            ),
            (
                "/Contents/[a]pendics.shuffle/Framed And Defamed/04 B2 - In Charge (Stavöstranos Peep Mix).mp3",
                "/PIONEER/USBANLZ/P06D/000161C3/ANLZ0000.DAT",
            ),
        ] {
            assert_eq!(analysis_path(path).to_string(), expected, "{path}");
        }
    }

    #[test]
    fn a_numbered_path_keeps_its_folder() {
        let path = analysis_path("/Contents/UnknownArtist/UnknownAlbum/HORN.wav").numbered(1);
        assert_eq!(
            path.file(AnalysisFileKind::TwoEx),
            "/PIONEER/USBANLZ/P021/00006D2B/ANLZ0001.2EX"
        );
    }
}
