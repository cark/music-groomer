use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::file_copy;

pub const PARTIAL_DIRECTORY: &str = ".music-groomer-partials";
pub const PARTIAL_MARKER: &str = ".music-groomer-partial";
pub const MARKER_CONTENTS: &[u8] = b"music-groomer publication partial v1\n";

static NEXT_PARTIAL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationRoute {
    DirectRename,
    DestinationCopy,
}

#[derive(Debug)]
pub struct PublicationResult {
    pub route: PublicationRoute,
    pub cleanup_warning: Option<String>,
}

pub fn publish(
    stage: &Path,
    destination_root: &Path,
    destination: &Path,
    force_destination_copy: bool,
    mut validate_destination_copy: impl FnMut(&Path) -> Result<(), String>,
) -> Result<PublicationResult, PublicationError> {
    if destination
        .try_exists()
        .map_err(|source| PublicationError::Io {
            path: destination.to_owned(),
            source,
        })?
    {
        return Err(PublicationError::Collision(destination.to_owned()));
    }
    let created_parents = create_parents(destination_root, destination)?;
    let direct = !force_destination_copy && same_filesystem(stage, destination_root)?;
    let result = if direct {
        exclusive_rename(stage, destination).map(|()| PublicationResult {
            route: PublicationRoute::DirectRename,
            cleanup_warning: None,
        })
    } else {
        publish_through_partial(
            stage,
            destination_root,
            destination,
            &mut validate_destination_copy,
        )
    };
    if result.is_err() {
        remove_empty_parents(created_parents);
    }
    result
}

fn publish_through_partial(
    stage: &Path,
    destination_root: &Path,
    destination: &Path,
    validate: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> Result<PublicationResult, PublicationError> {
    let partial_root = destination_root.join(PARTIAL_DIRECTORY);
    fs::create_dir_all(&partial_root).map_err(|source| PublicationError::Io {
        path: partial_root.clone(),
        source,
    })?;
    let entry = create_partial_entry(&partial_root)?;
    let payload = entry.join("result");
    let operation = (|| {
        write_marker(&entry)?;
        copy_tree(stage, &payload)?;
        validate(&payload).map_err(|cause| PublicationError::Validation {
            path: payload.clone(),
            cause,
        })?;
        exclusive_rename(&payload, destination)?;
        Ok(PublicationResult {
            route: PublicationRoute::DestinationCopy,
            cleanup_warning: None,
        })
    })();
    match operation {
        Ok(mut result) => {
            result.cleanup_warning = remove_partial_container(&entry, &partial_root);
            Ok(result)
        }
        Err(original) => match remove_marked_entry(&entry) {
            Ok(()) => {
                let _ = fs::remove_dir(&partial_root);
                Err(original)
            }
            Err(source) => Err(PublicationError::Cleanup {
                original: Box::new(original),
                path: entry,
                source,
            }),
        },
    }
}

fn create_partial_entry(root: &Path) -> Result<PathBuf, PublicationError> {
    for _ in 0..100 {
        let sequence = NEXT_PARTIAL.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("partial-{}-{sequence}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(PublicationError::Io { path, source }),
        }
    }
    Err(PublicationError::CannotAllocatePartial(root.to_owned()))
}

fn write_marker(entry: &Path) -> Result<(), PublicationError> {
    let path = entry.join(PARTIAL_MARKER);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| PublicationError::Io {
            path: path.clone(),
            source,
        })?;
    file.write_all(MARKER_CONTENTS)
        .map_err(|source| PublicationError::Io { path, source })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), PublicationError> {
    fs::create_dir(destination).map_err(|source| PublicationError::Io {
        path: destination.to_owned(),
        source,
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| PublicationError::Io {
            path: source.to_owned(),
            source: error,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PublicationError::Io {
            path: source.to_owned(),
            source: error,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source| PublicationError::Io {
                path: source_path.clone(),
                source,
            })?;
        if metadata.file_type().is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.file_type().is_file() {
            file_copy::copy_contents(&source_path, &destination_path).map_err(|source| {
                PublicationError::Io {
                    path: destination_path,
                    source,
                }
            })?;
        } else {
            return Err(PublicationError::UnsafeObject(source_path));
        }
    }
    Ok(())
}

pub(crate) fn create_parents(
    destination_root: &Path,
    destination: &Path,
) -> Result<Vec<PathBuf>, PublicationError> {
    let parent = destination
        .parent()
        .ok_or_else(|| PublicationError::OutsideRoot(destination.to_owned()))?;
    if !parent.starts_with(destination_root) {
        return Err(PublicationError::OutsideRoot(destination.to_owned()));
    }
    let mut missing = Vec::new();
    let mut current = parent;
    while current != destination_root && !current.exists() {
        missing.push(current.to_owned());
        current = current
            .parent()
            .ok_or_else(|| PublicationError::OutsideRoot(destination.to_owned()))?;
    }
    missing.reverse();
    let mut created = Vec::new();
    for path in missing {
        if let Err(source) = fs::create_dir(&path) {
            remove_empty_parents(created);
            return Err(PublicationError::Io { path, source });
        }
        created.push(path);
    }
    Ok(created)
}

pub(crate) fn remove_empty_parents(mut paths: Vec<PathBuf>) {
    paths.reverse();
    for path in paths {
        let _ = fs::remove_dir(path);
    }
}

fn remove_partial_container(entry: &Path, root: &Path) -> Option<String> {
    let operations = [
        (entry.join(PARTIAL_MARKER), true),
        (entry.to_owned(), false),
        (root.to_owned(), false),
    ];
    for (path, file) in operations {
        let result = if file {
            fs::remove_file(&path)
        } else {
            fs::remove_dir(&path)
        };
        if let Err(error) = result {
            return Some(format!("could not remove {}: {error}", path.display()));
        }
    }
    None
}

fn remove_marked_entry(entry: &Path) -> std::io::Result<()> {
    let marker = entry.join(PARTIAL_MARKER);
    let metadata = fs::symlink_metadata(&marker)?;
    if !metadata.file_type().is_file() || fs::read(&marker)? != MARKER_CONTENTS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "publication marker is missing or invalid",
        ));
    }
    fs::remove_dir_all(entry)
}

#[cfg(unix)]
fn same_filesystem(left: &Path, right: &Path) -> Result<bool, PublicationError> {
    use std::os::unix::fs::MetadataExt;
    let left_metadata = fs::metadata(left).map_err(|source| PublicationError::Io {
        path: left.to_owned(),
        source,
    })?;
    let right_metadata = fs::metadata(right).map_err(|source| PublicationError::Io {
        path: right.to_owned(),
        source,
    })?;
    Ok(left_metadata.dev() == right_metadata.dev())
}

#[cfg(windows)]
fn same_filesystem(left: &Path, right: &Path) -> Result<bool, PublicationError> {
    Ok(windows_volume(left)? == windows_volume(right)?)
}

#[cfg(windows)]
fn windows_volume(path: &Path) -> Result<Vec<u16>, PublicationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
    let canonical = path.canonicalize().map_err(|source| PublicationError::Io {
        path: path.to_owned(),
        source,
    })?;
    let input = canonical
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut output = vec![0_u16; 32_768];
    // SAFETY: input is a live NUL-terminated UTF-16 path and output is a
    // writable buffer whose length is passed exactly.
    let found = unsafe {
        GetVolumePathNameW(
            input.as_ptr(),
            output.as_mut_ptr(),
            u32::try_from(output.len()).expect("fixed buffer length fits u32"),
        )
    };
    if found == 0 {
        return Err(PublicationError::Io {
            path: canonical,
            source: std::io::Error::last_os_error(),
        });
    }
    let length = output
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(output.len());
    output.truncate(length);
    for unit in &mut output {
        if (*unit >= u16::from(b'A')) && (*unit <= u16::from(b'Z')) {
            *unit += u16::from(b'a' - b'A');
        }
    }
    Ok(output)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn exclusive_rename(source: &Path, destination: &Path) -> Result<(), PublicationError> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            PublicationError::Collision(destination.to_owned())
        } else {
            PublicationError::Io {
                path: destination.to_owned(),
                source: std::io::Error::from_raw_os_error(error.raw_os_error()),
            }
        }
    })
}

#[cfg(windows)]
pub(crate) fn exclusive_rename(source: &Path, destination: &Path) -> Result<(), PublicationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;
    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both buffers are live, immutable, NUL-terminated UTF-16 paths.
    let moved = unsafe { MoveFileW(source_wide.as_ptr(), destination_wide.as_ptr()) };
    if moved != 0 {
        return Ok(());
    }
    let source_error = std::io::Error::last_os_error();
    if destination.exists() {
        Err(PublicationError::Collision(destination.to_owned()))
    } else {
        Err(PublicationError::Io {
            path: destination.to_owned(),
            source: source_error,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn exclusive_rename(_source: &Path, destination: &Path) -> Result<(), PublicationError> {
    Err(PublicationError::UnsupportedPlatform(
        destination.to_owned(),
    ))
}

#[derive(Debug)]
pub enum PublicationError {
    Collision(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Cleanup {
        original: Box<PublicationError>,
        path: PathBuf,
        source: std::io::Error,
    },
    OutsideRoot(PathBuf),
    UnsafeObject(PathBuf),
    CannotAllocatePartial(PathBuf),
    Validation {
        path: PathBuf,
        cause: String,
    },
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    UnsupportedPlatform(PathBuf),
}

impl PublicationError {
    pub fn path(&self) -> &Path {
        match self {
            Self::Collision(path)
            | Self::Io { path, .. }
            | Self::Cleanup { path, .. }
            | Self::OutsideRoot(path)
            | Self::UnsafeObject(path)
            | Self::CannotAllocatePartial(path)
            | Self::Validation { path, .. } => path,
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
            Self::UnsupportedPlatform(path) => path,
        }
    }

    pub fn cleanup_failure(&self) -> Option<String> {
        match self {
            Self::Cleanup { path, source, .. } => Some(format!(
                "publication partial remains at {}: {source}",
                path.display()
            )),
            _ => None,
        }
    }
}

impl fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collision(path) => write!(
                formatter,
                "the final release path already exists and was not overwritten: {}",
                path.display()
            ),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Cleanup {
                original,
                path,
                source,
            } => write!(
                formatter,
                "{original}; also could not clean publication partial {}: {source}",
                path.display()
            ),
            Self::OutsideRoot(path) => write!(
                formatter,
                "destination {} is outside the selected destination root",
                path.display()
            ),
            Self::UnsafeObject(path) => write!(
                formatter,
                "staging contains an unsafe filesystem object: {}",
                path.display()
            ),
            Self::CannotAllocatePartial(path) => write!(
                formatter,
                "cannot allocate a unique publication partial under {}",
                path.display()
            ),
            Self::Validation { path, cause } => write!(
                formatter,
                "destination-side publication copy failed validation at {}: {cause}",
                path.display()
            ),
            #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
            Self::UnsupportedPlatform(path) => write!(
                formatter,
                "this platform has no exclusive publication primitive for {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PublicationError {}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn direct_publication_renames_complete_directory() {
        let temporary = TempDir::new().unwrap();
        let stage = temporary.path().join("stage");
        let root = temporary.path().join("library");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(stage.join("track.flac"), b"audio").unwrap();
        let destination = root.join("Artist/Album");

        let result = publish(&stage, &root, &destination, false, |_| Ok(())).unwrap();

        assert_eq!(result.route, PublicationRoute::DirectRename);
        assert_eq!(fs::read(destination.join("track.flac")).unwrap(), b"audio");
        assert!(!stage.exists());
    }

    #[test]
    fn forced_copy_route_publishes_and_removes_partial_data() {
        let temporary = TempDir::new().unwrap();
        let stage = temporary.path().join("stage");
        let root = temporary.path().join("library");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(stage.join("track.flac"), b"audio").unwrap();
        let destination = root.join("Artist/Album");

        let validated = Cell::new(false);
        let result = publish(&stage, &root, &destination, true, |payload| {
            validated.set(true);
            fs::read(payload.join("track.flac"))
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();

        assert_eq!(result.route, PublicationRoute::DestinationCopy);
        assert_eq!(fs::read(destination.join("track.flac")).unwrap(), b"audio");
        assert!(!root.join(PARTIAL_DIRECTORY).exists());
        assert!(stage.exists());
        assert!(validated.get());
    }

    #[test]
    fn failed_destination_copy_validation_publishes_nothing_and_cleans_partial() {
        let temporary = TempDir::new().unwrap();
        let stage = temporary.path().join("stage");
        let root = temporary.path().join("library");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(stage.join("track.flac"), b"audio").unwrap();
        let destination = root.join("Artist/Album");

        let error = publish(&stage, &root, &destination, true, |_| {
            Err("injected mismatch".into())
        })
        .unwrap_err();

        assert!(matches!(error, PublicationError::Validation { .. }));
        assert!(!destination.exists());
        assert!(!root.join(PARTIAL_DIRECTORY).exists());
        assert!(stage.exists());
    }

    #[test]
    fn destination_created_before_atomic_rename_survives_untouched() {
        let temporary = TempDir::new().unwrap();
        let stage = temporary.path().join("stage");
        let root = temporary.path().join("library");
        let destination = root.join("Artist/Album");
        fs::create_dir(&stage).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(stage.join("ours"), b"ours").unwrap();
        fs::write(destination.join("theirs"), b"theirs").unwrap();

        let error = publish(&stage, &root, &destination, false, |_| Ok(())).unwrap_err();

        assert!(matches!(error, PublicationError::Collision(_)));
        assert_eq!(fs::read(destination.join("theirs")).unwrap(), b"theirs");
        assert!(stage.exists());
    }
}
