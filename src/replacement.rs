use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::SourceKind;
use crate::plan::GroomingPlan;
use crate::recovery::RECOVERY_DIRECTORY;
use crate::source::SourceInspection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementContext {
    pub library_root: PathBuf,
    pub active_path: PathBuf,
    pub historical_path: PathBuf,
    pub destination: PathBuf,
}

pub fn detect(
    source: &SourceInspection,
    plan: &GroomingPlan,
) -> Result<Option<ReplacementContext>, ReplacementError> {
    if source.kind != SourceKind::AlbumDirectory {
        return Ok(None);
    }

    let source_metadata = fs::symlink_metadata(&source.source)
        .map_err(|source_error| ReplacementError::Io(source.source.clone(), source_error))?;
    if !source_metadata.file_type().is_dir() {
        return Err(ReplacementError::UnsafeSource(source.source.clone()));
    }
    let library_root = canonical_directory(&plan.destination_root)?;
    let active_path = canonical_directory(&source.source)?;
    let historical_path = match active_path.strip_prefix(&library_root) {
        Ok(path) if !path.as_os_str().is_empty() => path.to_owned(),
        _ => return Ok(None),
    };
    if historical_path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == RECOVERY_DIRECTORY)
    {
        return Err(ReplacementError::RecoverySource(active_path));
    }

    let relative_destination = plan
        .destination
        .strip_prefix(&plan.destination_root)
        .map_err(|_| ReplacementError::DestinationOutsideRoot(plan.destination.clone()))?;
    if relative_destination.as_os_str().is_empty() {
        return Err(ReplacementError::DestinationOutsideRoot(
            plan.destination.clone(),
        ));
    }
    let destination = library_root.join(relative_destination);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(ReplacementError::ExternalCollision(destination));
            }
            let canonical_destination = destination
                .canonicalize()
                .map_err(|source_error| ReplacementError::Io(destination.clone(), source_error))?;
            if canonical_destination != active_path {
                return Err(ReplacementError::ExternalCollision(destination));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source_error) => {
            return Err(ReplacementError::Io(destination.clone(), source_error));
        }
    }

    Ok(Some(ReplacementContext {
        library_root,
        active_path,
        historical_path,
        destination,
    }))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, ReplacementError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| ReplacementError::Io(path.to_owned(), source))?;
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|source| ReplacementError::Io(canonical.clone(), source))?;
    if !metadata.file_type().is_dir() {
        return Err(ReplacementError::UnsafeSource(path.to_owned()));
    }
    Ok(canonical)
}

#[derive(Debug)]
pub enum ReplacementError {
    UnsafeSource(PathBuf),
    RecoverySource(PathBuf),
    DestinationOutsideRoot(PathBuf),
    ExternalCollision(PathBuf),
    Io(PathBuf, std::io::Error),
}

impl fmt::Display for ReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeSource(path) => write!(
                formatter,
                "replacement requires an ordinary selected release directory: {}",
                path.display()
            ),
            Self::RecoverySource(path) => write!(
                formatter,
                "refusing to groom retained recovery content directly: {}",
                path.display()
            ),
            Self::DestinationOutsideRoot(path) => write!(
                formatter,
                "planned destination is outside the selected library root: {}",
                path.display()
            ),
            Self::ExternalCollision(path) => write!(
                formatter,
                "the canonical replacement path is occupied by unrelated content: {}",
                path.display()
            ),
            Self::Io(path, source) => write!(formatter, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ReplacementError {}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::plan::{ArtworkChoice, ArtworkOrigin, GroomingPlan, MatchSelection, MetadataBasis};

    #[test]
    fn selected_library_directory_establishes_replacement_without_a_collision() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let active = library.join("Artist/Old Album");
        fs::create_dir_all(&active).unwrap();
        fs::write(active.join("track.flac"), b"not inspected in this unit").unwrap();
        let source = inspection(&active, SourceKind::AlbumDirectory);
        let plan = plan(&library, library.join("Artist/New Album"));

        let context = detect(&source, &plan).unwrap().unwrap();

        assert_eq!(context.active_path, active);
        assert_eq!(context.historical_path, Path::new("Artist/Old Album"));
        assert_eq!(context.destination, library.join("Artist/New Album"));
    }

    #[test]
    fn existing_different_destination_is_never_inferred_as_replacement() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let active = library.join("Artist/Old Album");
        let occupied = library.join("Artist/New Album");
        fs::create_dir_all(&active).unwrap();
        fs::create_dir_all(&occupied).unwrap();
        let source = inspection(&active, SourceKind::AlbumDirectory);

        let error = detect(&source, &plan(&library, occupied.clone())).unwrap_err();

        assert!(matches!(error, ReplacementError::ExternalCollision(path) if path == occupied));
    }

    #[test]
    fn loose_library_file_does_not_claim_its_directory() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let source_path = library.join("track.flac");
        fs::write(&source_path, b"not inspected in this unit").unwrap();
        let source = inspection(&source_path, SourceKind::LooseFile);

        assert!(
            detect(&source, &plan(&library, library.join("Artist/Album")))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn recovery_content_cannot_become_an_active_replacement_source() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let retained = library.join(RECOVERY_DIRECTORY).join("payload");
        fs::create_dir_all(&retained).unwrap();
        let source = inspection(&retained, SourceKind::AlbumDirectory);

        let error = detect(&source, &plan(&library, library.join("Artist/Album"))).unwrap_err();

        assert!(matches!(error, ReplacementError::RecoverySource(path) if path == retained));
    }

    fn inspection(path: &Path, kind: SourceKind) -> SourceInspection {
        SourceInspection {
            source: path.to_owned(),
            kind,
            audio: Vec::new(),
            ancillary: Vec::new(),
            artwork: Vec::new(),
            selected_artwork: None,
            notices: Vec::new(),
            snapshot: Vec::new(),
        }
    }

    fn plan(library: &Path, destination: PathBuf) -> GroomingPlan {
        GroomingPlan {
            source_label: "source".into(),
            metadata: MetadataBasis::ExistingTags,
            match_selection: MatchSelection::ExistingTags,
            match_reasons: Vec::new(),
            destination_root: library.to_owned(),
            destination,
            tracks: Vec::new(),
            ancillary: Vec::new(),
            ancillary_directories: Vec::new(),
            artwork: ArtworkChoice {
                origin: ArtworkOrigin::None,
                label: "none".into(),
                dimensions: None,
                output_name: None,
            },
            warnings: Vec::new(),
            preserved_embedded_artwork: 0,
            archive_artwork_bytes: None,
        }
    }
}
