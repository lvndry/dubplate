//! The stick or folder an export goes to: where each file lives on it, what is already there,
//! and Dubplate's record of the files it wrote.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dubplate_rekordbox::device_library::DeviceLibrary;
use serde::{Deserialize, Serialize};

use crate::error::UsbExportError;
use crate::files::write_atomically;

const PIONEER: &str = "PIONEER";
const HIDDEN_PIONEER: &str = ".PIONEER";
const REKORDBOX: &str = "rekordbox";
const EXPORT_PDB: &str = "export.pdb";
const EXPORT_EXT_PDB: &str = "exportExt.pdb";
const ONE_LIBRARY_FILES: [&str; 4] = [
    "exportLibrary.db",
    "exportLibrary.db-wal",
    "exportLibrary.db-shm",
    "exportLibrary.db-journal",
];
const MANIFEST: &str = "dubplate-export.json";
const MANIFEST_FORMAT: u32 = 1;
/// The files players and rekordbox keep in `PIONEER` for the players' settings, which say
/// nothing about a library and which Dubplate leaves alone.
const SETTING_FILES: [&str; 4] = [
    "DEVSETTING.DAT",
    "MYSETTING.DAT",
    "MYSETTING2.DAT",
    "DJMMYSETTING.DAT",
];

/// The root of the stick or folder an export goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stick {
    root: PathBuf,
}

/// What Dubplate found on the stick before writing.
#[derive(Debug)]
pub(crate) enum StickLibrary {
    /// No library: Dubplate starts one.
    Empty,
    /// A library Dubplate wrote before, which it adds to.
    Dubplate {
        /// The library as read.
        library: Box<DeviceLibrary>,
        /// Whether `exportExt.pdb` is there, possibly with players' changes, to be kept.
        has_export_ext: bool,
    },
}

impl Stick {
    /// The stick or folder at `root`, once checked: it must be a folder, outside rekordbox's
    /// folders.
    pub(crate) fn open(root: &Path) -> Result<Self, UsbExportError> {
        if !root.is_dir() {
            return Err(UsbExportError::NotAFolder(root.to_path_buf()));
        }
        let canonical = root.canonicalize().map_err(UsbExportError::io(root))?;
        if [root, canonical.as_path()]
            .into_iter()
            .any(dubplate_library::is_inside_rekordbox)
        {
            return Err(UsbExportError::InsideRekordbox(root.to_path_buf()));
        }
        Ok(Self { root: canonical })
    }

    /// The stick's root.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The file at `device_path`, a path as players see it, such as `/Contents/A/B/C.mp3`.
    pub(crate) fn file(&self, device_path: &str) -> PathBuf {
        device_path
            .trim_start_matches('/')
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    pub(crate) fn export_pdb(&self) -> PathBuf {
        self.rekordbox_folder().join(EXPORT_PDB)
    }

    pub(crate) fn export_ext_pdb(&self) -> PathBuf {
        self.rekordbox_folder().join(EXPORT_EXT_PDB)
    }

    fn rekordbox_folder(&self) -> PathBuf {
        self.root.join(PIONEER).join(REKORDBOX)
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join(PIONEER).join(MANIFEST)
    }

    /// The device paths of the library files, as the manifest lists them.
    pub(crate) fn library_files() -> [String; 2] {
        [
            format!("/{PIONEER}/{REKORDBOX}/{EXPORT_PDB}"),
            format!("/{PIONEER}/{REKORDBOX}/{EXPORT_EXT_PDB}"),
        ]
    }

    /// Reads what is on the stick: nothing, a library Dubplate wrote, or a library another
    /// program wrote, which is an error since Dubplate does not merge into it.
    pub(crate) fn library(&self) -> Result<(StickLibrary, Manifest), UsbExportError> {
        let foreign = |found: String| UsbExportError::ForeignLibrary {
            destination: self.root.clone(),
            found,
        };
        if self.root.join(HIDDEN_PIONEER).exists() {
            return Err(foreign(format!("a {HIDDEN_PIONEER} folder")));
        }
        let pioneer = self.root.join(PIONEER);
        if !pioneer.exists() {
            return Ok((StickLibrary::Empty, Manifest::default()));
        }
        if let Some(found) = one_library_file(&pioneer)? {
            return Err(UsbExportError::OneLibrary {
                destination: self.root.clone(),
                found,
            });
        }
        let manifest_path = self.manifest_path();
        if !manifest_path.is_file() {
            let found = foreign_entries(&pioneer)?;
            return if found.is_empty() {
                Ok((StickLibrary::Empty, Manifest::default()))
            } else {
                Err(foreign(format!("{} in {PIONEER}", found.join(", "))))
            };
        }
        let manifest = Manifest::read(&manifest_path)?;
        let export_pdb = self.export_pdb();
        let library = match std::fs::read(&export_pdb) {
            Ok(bytes) => DeviceLibrary::parse_export_pdb(&bytes).map_err(|source| {
                UsbExportError::UnreadableLibrary {
                    path: export_pdb.clone(),
                    source,
                }
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((StickLibrary::Empty, manifest));
            }
            Err(source) => {
                return Err(UsbExportError::Io {
                    path: export_pdb,
                    source,
                });
            }
        };
        Ok((
            StickLibrary::Dubplate {
                library: Box::new(library),
                has_export_ext: self.export_ext_pdb().is_file(),
            },
            manifest,
        ))
    }

    /// Writes `manifest` to the stick.
    pub(crate) fn write_manifest(&self, manifest: &Manifest) -> Result<(), UsbExportError> {
        let path = self.manifest_path();
        let json = serde_json::to_vec_pretty(manifest).map_err(|error| {
            UsbExportError::UnreadableManifest {
                path: path.clone(),
                reason: error.to_string(),
            }
        })?;
        write_atomically(&path, &json)
    }
}

/// Detect either the OneLibrary catalog or a sidecar from a OneLibrary transaction.
fn one_library_file(pioneer: &Path) -> Result<Option<String>, UsbExportError> {
    for entry in std::fs::read_dir(pioneer).map_err(UsbExportError::io(pioneer))? {
        let entry = entry.map_err(UsbExportError::io(pioneer))?;
        if !entry
            .file_name()
            .eq_ignore_ascii_case(std::ffi::OsStr::new(REKORDBOX))
        {
            continue;
        }
        let rekordbox = entry.path();
        for file in std::fs::read_dir(&rekordbox).map_err(UsbExportError::io(&rekordbox))? {
            let file = file.map_err(UsbExportError::io(&rekordbox))?;
            let name = file.file_name();
            if ONE_LIBRARY_FILES
                .iter()
                .any(|marker| name.eq_ignore_ascii_case(std::ffi::OsStr::new(marker)))
            {
                return Ok(Some(name.to_string_lossy().into_owned()));
            }
        }
    }
    Ok(None)
}

/// The entries of `PIONEER` other than players' setting files.
fn foreign_entries(pioneer: &Path) -> Result<Vec<String>, UsbExportError> {
    let entries = std::fs::read_dir(pioneer).map_err(UsbExportError::io(pioneer))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(UsbExportError::io(pioneer))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_setting = SETTING_FILES
            .iter()
            .any(|setting| setting.eq_ignore_ascii_case(&name));
        if !is_setting && !name.starts_with("._") && name != ".DS_Store" {
            found.push(name);
        }
    }
    found.sort();
    Ok(found)
}

/// Dubplate's record of the files it wrote to a stick, kept at `PIONEER/dubplate-export.json`.
/// A stick with this record holds a library Dubplate may add to; a file it does not list was
/// not written by Dubplate and is never replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    format: u32,
    written_by: String,
    files: BTreeSet<String>,
}

impl Manifest {
    fn read(path: &Path) -> Result<Self, UsbExportError> {
        let bytes = std::fs::read(path).map_err(UsbExportError::io(path))?;
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|error| UsbExportError::UnreadableManifest {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
        if manifest.format != MANIFEST_FORMAT {
            return Err(UsbExportError::UnreadableManifest {
                path: path.to_path_buf(),
                reason: format!(
                    "it is in format {}, which this version of Dubplate does not know",
                    manifest.format
                ),
            });
        }
        Ok(manifest)
    }

    /// Whether Dubplate wrote the file at `device_path`.
    pub(crate) fn wrote(&self, device_path: &str) -> bool {
        self.files.contains(device_path)
    }

    /// Records that Dubplate writes the file at `device_path`.
    pub(crate) fn record(&mut self, device_path: impl Into<String>) {
        self.format = MANIFEST_FORMAT;
        self.written_by = format!("Dubplate {}", env!("CARGO_PKG_VERSION"));
        self.files.insert(device_path.into());
    }
}
