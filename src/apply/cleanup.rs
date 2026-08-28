use std::fs;
use std::path::{Path, PathBuf};

use super::publication::{MARKER_CONTENTS, PARTIAL_DIRECTORY, PARTIAL_MARKER};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbandonedPartial {
    pub path: PathBuf,
    pub bytes: u64,
}

pub fn find(destination_root: &Path) -> Result<Vec<AbandonedPartial>, CleanupError> {
    let root = destination_root.join(PARTIAL_DIRECTORY);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(CleanupError::Io { path: root, source }),
    };
    let mut partials = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CleanupError::Io {
            path: root.clone(),
            source,
        })?;
        let path = entry.path();
        if valid_marker(&path)? {
            partials.push(AbandonedPartial {
                bytes: tree_size(&path)?,
                path,
            });
        }
    }
    partials.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(partials)
}

pub fn remove(partial: &AbandonedPartial) -> Result<(), CleanupError> {
    if !valid_marker(&partial.path)? {
        return Err(CleanupError::Unowned(partial.path.clone()));
    }
    fs::remove_dir_all(&partial.path).map_err(|source| CleanupError::Io {
        path: partial.path.clone(),
        source,
    })?;
    if let Some(root) = partial.path.parent() {
        let _ = fs::remove_dir(root);
    }
    Ok(())
}

fn valid_marker(entry: &Path) -> Result<bool, CleanupError> {
    let metadata = match fs::symlink_metadata(entry) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(CleanupError::Io {
                path: entry.to_owned(),
                source,
            });
        }
    };
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }
    let marker = entry.join(PARTIAL_MARKER);
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(CleanupError::Io {
                path: marker,
                source,
            });
        }
    };
    if !metadata.file_type().is_file() {
        return Ok(false);
    }
    fs::read(&marker)
        .map(|contents| contents == MARKER_CONTENTS)
        .map_err(|source| CleanupError::Io {
            path: marker,
            source,
        })
}

fn tree_size(path: &Path) -> Result<u64, CleanupError> {
    let mut total = 0_u64;
    let entries = fs::read_dir(path).map_err(|source| CleanupError::Io {
        path: path.to_owned(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CleanupError::Io {
            path: path.to_owned(),
            source,
        })?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child).map_err(|source| CleanupError::Io {
            path: child.clone(),
            source,
        })?;
        if metadata.file_type().is_dir() {
            total = total.saturating_add(tree_size(&child)?);
        } else if metadata.file_type().is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

#[derive(Debug)]
pub enum CleanupError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Unowned(PathBuf),
}

impl std::fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Unowned(path) => write!(
                formatter,
                "refusing to remove unmarked publication data at {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CleanupError {}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn only_marked_direct_children_are_offered_and_removed() {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path();
        let partial_root = root.join(PARTIAL_DIRECTORY);
        let owned = partial_root.join("owned");
        let foreign = partial_root.join("foreign");
        fs::create_dir_all(owned.join("result")).unwrap();
        fs::create_dir(&foreign).unwrap();
        fs::write(owned.join(PARTIAL_MARKER), MARKER_CONTENTS).unwrap();
        fs::write(owned.join("result/track"), b"12345").unwrap();

        let found = find(root).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, owned);
        assert!(found[0].bytes >= 5);
        remove(&found[0]).unwrap();
        assert!(!owned.exists());
        assert!(foreign.exists());
    }

    #[test]
    fn removal_rechecks_the_marker() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join(PARTIAL_DIRECTORY).join("partial");
        fs::create_dir_all(&path).unwrap();
        let partial = AbandonedPartial { path, bytes: 0 };

        let error = remove(&partial).unwrap_err();

        assert!(matches!(error, CleanupError::Unowned(_)));
    }
}
