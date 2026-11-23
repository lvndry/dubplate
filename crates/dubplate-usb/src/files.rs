//! Writing files on the stick so that an unplugged stick or a full one never leaves half a file
//! where a whole one was: every file is written beside its place, flushed, then moved there.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::UsbExportError;

const PARTIAL_SUFFIX: &str = ".dubplate-partial";
const COMPARE_CHUNK: usize = 1 << 20;

fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(PARTIAL_SUFFIX);
    path.with_file_name(name)
}

fn create_parent(path: &Path) -> Result<(), UsbExportError> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(UsbExportError::io(folder))?;
    }
    Ok(())
}

/// Writes `bytes` to `path` through a file beside it, flushed to the stick before it takes the
/// place of what was there.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), UsbExportError> {
    create_parent(path)?;
    let partial = partial_path(path);
    let result = (|| {
        let mut file = File::create(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    finish(result, &partial, path)
}

/// Copies the file `from` to `to` through a file beside it, flushed before it takes its place.
pub(crate) fn copy_atomically(from: &Path, to: &Path) -> Result<(), UsbExportError> {
    create_parent(to)?;
    let partial = partial_path(to);
    let mut source = File::open(from).map_err(UsbExportError::io(from))?;
    let result = (|| {
        let mut file = File::create(&partial)?;
        std::io::copy(&mut source, &mut file)?;
        file.sync_all()
    })();
    finish(result, &partial, to)
}

fn finish(result: std::io::Result<()>, partial: &Path, path: &Path) -> Result<(), UsbExportError> {
    if let Err(source) = result {
        let _ = std::fs::remove_file(partial);
        return Err(UsbExportError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    std::fs::rename(partial, path).map_err(|source| {
        let _ = std::fs::remove_file(partial);
        UsbExportError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// Whether the files at `first` and `second` hold the same bytes.
pub fn same_contents(first: &Path, second: &Path) -> Result<bool, UsbExportError> {
    let length = |path: &Path| {
        std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .map_err(UsbExportError::io(path))
    };
    if length(first)? != length(second)? {
        return Ok(false);
    }
    let mut first_file = File::open(first).map_err(UsbExportError::io(first))?;
    let mut second_file = File::open(second).map_err(UsbExportError::io(second))?;
    let mut first_chunk = vec![0_u8; COMPARE_CHUNK];
    let mut second_chunk = vec![0_u8; COMPARE_CHUNK];
    loop {
        let read =
            read_full(&mut first_file, &mut first_chunk).map_err(UsbExportError::io(first))?;
        let other =
            read_full(&mut second_file, &mut second_chunk).map_err(UsbExportError::io(second))?;
        if read != other || first_chunk[..read] != second_chunk[..other] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

fn read_full(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_leaves_nothing_beside_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("deep").join("export.pdb");
        write_atomically(&path, b"first").unwrap();
        write_atomically(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn files_compare_by_their_bytes() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("a");
        let second = folder.path().join("b");
        std::fs::write(&first, vec![7_u8; 3 * COMPARE_CHUNK + 5]).unwrap();
        copy_atomically(&first, &second).unwrap();
        assert!(same_contents(&first, &second).unwrap());
        let mut changed = vec![7_u8; 3 * COMPARE_CHUNK + 5];
        changed[2 * COMPARE_CHUNK + 1] = 8;
        std::fs::write(&second, changed).unwrap();
        assert!(!same_contents(&first, &second).unwrap());
    }
}
