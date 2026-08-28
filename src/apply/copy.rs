use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::plan::{ArtworkOrigin, GroomingPlan};
use crate::planning::source_root;
use crate::source::{LoftyAudioReader, SourceInspection};

pub fn copy_to_stage(
    source: &SourceInspection,
    plan: &GroomingPlan,
    stage: &Path,
) -> Result<(), CopyError> {
    fs::create_dir(stage).map_err(|source| CopyError::io(stage, source))?;
    let source_root = source_root(source);
    let mut destinations = BTreeSet::new();
    for directory in &plan.ancillary_directories {
        let source_path = source_root.join(directory);
        let destination = stage.join(directory);
        fs::create_dir_all(&destination).map_err(|source| CopyError::io(&destination, source))?;
        let permissions = fs::metadata(&source_path)
            .map_err(|source| CopyError::io(&source_path, source))?
            .permissions();
        fs::set_permissions(&destination, permissions)
            .map_err(|source| CopyError::io(&destination, source))?;
    }
    for track in &plan.tracks {
        let relative = track
            .destination
            .strip_prefix(&plan.destination)
            .map_err(|_| CopyError::OutsideDestination(track.destination.clone()))?;
        copy_file(
            &source_root.join(&track.source_relative),
            &stage.join(relative),
            &mut destinations,
        )?;
    }
    for ancillary in &plan.ancillary {
        copy_file(
            &source_root.join(&ancillary.source_relative),
            &stage.join(&ancillary.destination_relative),
            &mut destinations,
        )?;
    }
    write_artwork(source, plan, stage, &source_root, &mut destinations)
}

pub fn groom(plan: &GroomingPlan, stage: &Path) -> Result<(), CopyError> {
    let reader = LoftyAudioReader;
    for track in &plan.tracks {
        let Some(tags) = &track.planned_tags else {
            continue;
        };
        let relative = track
            .destination
            .strip_prefix(&plan.destination)
            .map_err(|_| CopyError::OutsideDestination(track.destination.clone()))?;
        let path = stage.join(relative);
        reader
            .write_tags(&path, tags)
            .map_err(|source| CopyError::Tags { path, source })?;
    }
    Ok(())
}

fn write_artwork(
    source: &SourceInspection,
    plan: &GroomingPlan,
    stage: &Path,
    source_root: &Path,
    destinations: &mut BTreeSet<PathBuf>,
) -> Result<(), CopyError> {
    let Some(output_name) = &plan.artwork.output_name else {
        return Ok(());
    };
    let destination = stage.join(output_name);
    reserve(&destination, destinations)?;
    match &plan.artwork.origin {
        ArtworkOrigin::SourceSidecar { source_name } => {
            copy_file_unreserved(&source_root.join(source_name), &destination)
        }
        ArtworkOrigin::CoverArtArchive { .. } => {
            let bytes = plan
                .archive_artwork_bytes
                .as_ref()
                .ok_or(CopyError::MissingArtworkBytes)?;
            fs::write(&destination, bytes).map_err(|source| CopyError::io(&destination, source))
        }
        ArtworkOrigin::None => {
            let _ = source;
            Err(CopyError::MissingArtworkBytes)
        }
    }
}

fn copy_file(
    source: &Path,
    destination: &Path,
    destinations: &mut BTreeSet<PathBuf>,
) -> Result<(), CopyError> {
    reserve(destination, destinations)?;
    copy_file_unreserved(source, destination)
}

fn reserve(destination: &Path, destinations: &mut BTreeSet<PathBuf>) -> Result<(), CopyError> {
    if !destinations.insert(destination.to_owned()) {
        return Err(CopyError::Collision(destination.to_owned()));
    }
    Ok(())
}

fn copy_file_unreserved(source: &Path, destination: &Path) -> Result<(), CopyError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|source| CopyError::io(parent, source))?;
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| CopyError::Copy {
            source_path: source.to_owned(),
            destination: destination.to_owned(),
            source: error,
        })
}

#[derive(Debug)]
pub enum CopyError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Copy {
        source_path: PathBuf,
        destination: PathBuf,
        source: std::io::Error,
    },
    Tags {
        path: PathBuf,
        source: crate::source::AudioReadError,
    },
    OutsideDestination(PathBuf),
    Collision(PathBuf),
    MissingArtworkBytes,
}

impl CopyError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Io { path, .. } | Self::Tags { path, .. } => Some(path),
            Self::Copy { destination, .. } => Some(destination),
            Self::OutsideDestination(path) | Self::Collision(path) => Some(path),
            Self::MissingArtworkBytes => None,
        }
    }
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Copy {
                source_path,
                destination,
                source,
            } => write!(
                formatter,
                "cannot copy {} to {}: {source}",
                source_path.display(),
                destination.display()
            ),
            Self::Tags { path, source } => {
                write!(formatter, "cannot groom {}: {source}", path.display())
            }
            Self::OutsideDestination(path) => write!(
                formatter,
                "planned track {} is outside the album destination",
                path.display()
            ),
            Self::Collision(path) => write!(
                formatter,
                "more than one planned file would use {}",
                path.display()
            ),
            Self::MissingArtworkBytes => {
                formatter.write_str("the selected archive artwork has no planned bytes")
            }
        }
    }
}

impl std::error::Error for CopyError {}
