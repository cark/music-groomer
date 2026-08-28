use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::SourceKind;

use super::{SourceObjectKind, SourceSnapshotEntry};

#[derive(Debug)]
pub struct SnapshotError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot snapshot {}: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub fn capture(source: &Path, kind: SourceKind) -> Result<Vec<SourceSnapshotEntry>, SnapshotError> {
    let root = match kind {
        SourceKind::AlbumDirectory => source,
        SourceKind::LooseFile => source.parent().unwrap_or_else(|| Path::new("")),
    };
    let mut entries = Vec::new();
    capture_path(
        source,
        root,
        kind == SourceKind::AlbumDirectory,
        &mut entries,
    )?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

fn capture_path(
    path: &Path,
    root: &Path,
    recurse: bool,
    entries: &mut Vec<SourceSnapshotEntry>,
) -> Result<(), SnapshotError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SnapshotError {
        path: path.to_owned(),
        source,
    })?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        SourceObjectKind::File
    } else if file_type.is_dir() {
        SourceObjectKind::Directory
    } else if file_type.is_symlink() {
        SourceObjectKind::Symlink
    } else {
        SourceObjectKind::Special
    };
    entries.push(SourceSnapshotEntry {
        relative_path: path.strip_prefix(root).unwrap_or(path).to_owned(),
        kind,
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
    });

    if recurse && kind == SourceObjectKind::Directory {
        let directory = fs::read_dir(path).map_err(|source| SnapshotError {
            path: path.to_owned(),
            source,
        })?;
        let mut children =
            directory
                .collect::<Result<Vec<_>, _>>()
                .map_err(|source| SnapshotError {
                    path: path.to_owned(),
                    source,
                })?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            capture_path(&child.path(), root, true, entries)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn directory_snapshot_records_nested_objects_without_following_symlinks() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("album");
        fs::create_dir_all(source.join("disc")).unwrap();
        fs::write(source.join("disc/track.flac"), b"audio").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("disc", source.join("link")).unwrap();

        let snapshot = capture(&source, SourceKind::AlbumDirectory).unwrap();

        assert!(snapshot.iter().any(|entry| {
            entry.relative_path == Path::new("disc/track.flac")
                && entry.kind == SourceObjectKind::File
                && entry.bytes == 5
        }));
        #[cfg(unix)]
        assert!(snapshot.iter().any(|entry| {
            entry.relative_path == Path::new("link") && entry.kind == SourceObjectKind::Symlink
        }));
    }

    #[test]
    fn loose_snapshot_excludes_siblings() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("chosen.flac");
        fs::write(&source, b"chosen").unwrap();
        fs::write(temporary.path().join("sibling.flac"), b"sibling").unwrap();

        let snapshot = capture(&source, SourceKind::LooseFile).unwrap();

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].relative_path, Path::new("chosen.flac"));
    }
}
