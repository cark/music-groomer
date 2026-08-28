use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::domain::SourceKind;

use super::artwork::{self, ArtworkProbe};
use super::audio::{AudioProbe, AudioReadError, LoftyAudioReader};
use super::{AncillaryFile, ArtworkCandidate, InspectionNotice, NoticeKind, SourceInspection};

#[derive(Debug)]
pub enum InspectionError {
    SourceMetadata {
        path: PathBuf,
        error: std::io::Error,
    },
    InvalidSource(PathBuf),
    Progress(String),
}

impl fmt::Display for InspectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMetadata { path, error } => {
                write!(formatter, "cannot inspect {}: {error}", path.display())
            }
            Self::InvalidSource(path) => write!(
                formatter,
                "source {} must be an ordinary file or directory, not a symlink or special object",
                path.display()
            ),
            Self::Progress(error) => {
                write!(formatter, "cannot report inspection progress: {error}")
            }
        }
    }
}

impl std::error::Error for InspectionError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceInspector {
    audio: LoftyAudioReader,
}

pub trait InspectionProgress {
    fn inspecting_file(&mut self, path: &Path, number: usize, bytes: u64) -> Result<(), String>;
}

impl InspectionProgress for () {
    fn inspecting_file(&mut self, _path: &Path, _number: usize, _bytes: u64) -> Result<(), String> {
        Ok(())
    }
}

impl SourceInspector {
    pub fn inspect(&self, source: &Path) -> Result<SourceInspection, InspectionError> {
        self.inspect_with_progress(source, &mut ())
    }

    pub fn inspect_with_progress(
        &self,
        source: &Path,
        progress: &mut dyn InspectionProgress,
    ) -> Result<SourceInspection, InspectionError> {
        let span = tracing::info_span!("inspect_source", path = %source.display());
        let _entered = span.enter();
        let metadata =
            fs::symlink_metadata(source).map_err(|error| InspectionError::SourceMetadata {
                path: source.to_owned(),
                error,
            })?;
        let (kind, root) = if metadata.file_type().is_file() {
            (
                SourceKind::LooseFile,
                source.parent().unwrap_or(Path::new("")),
            )
        } else if metadata.file_type().is_dir() {
            (SourceKind::AlbumDirectory, source)
        } else {
            return Err(InspectionError::InvalidSource(source.to_owned()));
        };

        let mut inspection = SourceInspection {
            source: source.to_owned(),
            kind,
            audio: Vec::new(),
            ancillary: Vec::new(),
            artwork: Vec::new(),
            selected_artwork: None,
            notices: Vec::new(),
            snapshot: Vec::new(),
        };
        let mut inspected_files = 0;
        if kind == SourceKind::LooseFile {
            self.inspect_file(
                source,
                root,
                &mut inspection,
                progress,
                &mut inspected_files,
            )?;
        } else {
            self.inspect_directory(
                source,
                root,
                &mut inspection,
                progress,
                &mut inspected_files,
            )?;
        }
        inspection.audio.sort_by(|left, right| {
            left.relative_path
                .as_os_str()
                .cmp(right.relative_path.as_os_str())
        });
        inspection.ancillary.sort_by(|left, right| {
            left.relative_path
                .as_os_str()
                .cmp(right.relative_path.as_os_str())
        });
        tracing::debug_span!("analyze_source").in_scope(|| {
            super::analysis::finish(root, &mut inspection);
        });
        let snapshot = tracing::debug_span!("snapshot_source")
            .in_scope(|| super::snapshot::capture(source, kind));
        match snapshot {
            Ok(snapshot) => inspection.snapshot = snapshot,
            Err(error) => inspection.notices.push(InspectionNotice::blocker(
                NoticeKind::Unreadable,
                error.path.strip_prefix(root).ok().map(Path::to_owned),
                error.to_string(),
            )),
        }
        Ok(inspection)
    }

    fn inspect_directory(
        &self,
        directory: &Path,
        root: &Path,
        inspection: &mut SourceInspection,
        progress: &mut dyn InspectionProgress,
        inspected_files: &mut usize,
    ) -> Result<(), InspectionError> {
        let span = tracing::debug_span!("inspect_directory", path = %directory.display());
        let _entered = span.enter();
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    relative(directory, root),
                    format!("cannot read directory: {error}"),
                ));
                return Ok(());
            }
        };
        let mut entries = entries
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    relative(directory, root),
                    format!("cannot read a directory entry: {error}"),
                ));
                Vec::new()
            });
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let relative_path = relative(&path, root);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    inspection.notices.push(InspectionNotice::blocker(
                        NoticeKind::Unreadable,
                        relative_path,
                        format!("cannot inspect filesystem object: {error}"),
                    ));
                    continue;
                }
            };
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                inspection.notices.push(InspectionNotice::warning(
                    NoticeKind::SymlinkSkipped,
                    relative_path,
                    "symbolic link will not be followed or copied",
                ));
            } else if file_type.is_dir() {
                self.inspect_directory(&path, root, inspection, progress, inspected_files)?;
            } else if file_type.is_file() {
                self.inspect_file(&path, root, inspection, progress, inspected_files)?;
            } else {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::SpecialFile,
                    relative_path,
                    "special filesystem object cannot be preserved safely",
                ));
            }
        }
        Ok(())
    }

    fn inspect_file(
        &self,
        path: &Path,
        root: &Path,
        inspection: &mut SourceInspection,
        progress: &mut dyn InspectionProgress,
        inspected_files: &mut usize,
    ) -> Result<(), InspectionError> {
        let relative_path = relative(path, root).unwrap_or_else(|| path.to_owned());
        let metadata = match File::open(path).and_then(|file| file.metadata()) {
            Ok(metadata) => metadata,
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    Some(relative_path),
                    format!("cannot read file: {error}"),
                ));
                return Ok(());
            }
        };
        *inspected_files += 1;
        progress
            .inspecting_file(path, *inspected_files, metadata.len())
            .map_err(InspectionError::Progress)?;
        let span = tracing::trace_span!(
            "inspect_file",
            path = %path.display(),
            bytes = metadata.len()
        );
        let _entered = span.enter();
        let artwork_probe = artwork::probe(path);
        match &artwork_probe {
            Ok(ArtworkProbe::Supported { format, dimensions }) => {
                let is_root = relative_path
                    .parent()
                    .is_none_or(|parent| parent.as_os_str().is_empty());
                if is_root && let Some(name_priority) = artwork::name_priority(path) {
                    let actual_extension = path
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default();
                    if !extension_matches_artwork(actual_extension, *format) {
                        inspection.notices.push(InspectionNotice::warning(
                            NoticeKind::ArtworkExtensionMismatch,
                            Some(relative_path.clone()),
                            format!(
                                "{format} content will use canonical artwork name cover.{}",
                                format.canonical_extension()
                            ),
                        ));
                    }
                    inspection.artwork.push(ArtworkCandidate {
                        relative_path: relative_path.clone(),
                        format: *format,
                        dimensions: *dimensions,
                        name_priority,
                    });
                }
                inspection.ancillary.push(AncillaryFile {
                    relative_path,
                    bytes: metadata.len(),
                });
                tracing::trace!(kind = "image", format = %format, "file classified");
                return Ok(());
            }
            Ok(ArtworkProbe::RecognizedUnsupported(format)) => {
                inspection.notices.push(InspectionNotice::warning(
                    NoticeKind::UnsupportedImage,
                    Some(relative_path.clone()),
                    format!("{format} cannot be canonical artwork and will be preserved unchanged"),
                ));
                inspection.ancillary.push(AncillaryFile {
                    relative_path,
                    bytes: metadata.len(),
                });
                tracing::trace!(kind = "image", "file classified");
                return Ok(());
            }
            Ok(ArtworkProbe::ProbableUnsupported(_) | ArtworkProbe::NotImage) | Err(_) => {}
        }

        match self.audio.probe(path) {
            Ok(AudioProbe::Supported(audio)) => {
                let mut audio = *audio;
                audio.relative_path = relative_path.clone();
                inspect_audio_extension(&audio, path, inspection);
                tracing::trace!(kind = "audio", format = %audio.format, "file classified");
                inspection.audio.push(audio);
                return Ok(());
            }
            Ok(AudioProbe::Unsupported(format)) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::UnsupportedAudio,
                    Some(relative_path),
                    format!("recognized audio format {format} is not supported in v0.1"),
                ));
                return Ok(());
            }
            Ok(AudioProbe::NotAudio) => {}
            Err(AudioReadError::Parse(_)) if !probable_audio_extension(path) => {}
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    if probable_audio_extension(path) {
                        NoticeKind::CorruptAudio
                    } else {
                        NoticeKind::Unreadable
                    },
                    Some(relative_path),
                    error.to_string(),
                ));
                return Ok(());
            }
        }

        if probable_audio_extension(path) {
            inspection.notices.push(InspectionNotice::blocker(
                NoticeKind::UnsupportedAudio,
                Some(relative_path),
                "file looks like audio from its name but its contents are unsupported or damaged",
            ));
            return Ok(());
        }

        match artwork_probe {
            Ok(ArtworkProbe::ProbableUnsupported(format)) => {
                inspection.notices.push(InspectionNotice::warning(
                    NoticeKind::UnsupportedImage,
                    Some(relative_path.clone()),
                    format!("{format} cannot be canonical artwork and will be preserved unchanged"),
                ));
            }
            Ok(ArtworkProbe::NotImage) => {}
            Ok(ArtworkProbe::Supported { .. } | ArtworkProbe::RecognizedUnsupported(_)) => {
                unreachable!("recognized images return before audio inspection")
            }
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    Some(relative_path.clone()),
                    format!("cannot inspect probable artwork: {error}"),
                ));
            }
        }
        inspection.ancillary.push(AncillaryFile {
            relative_path,
            bytes: metadata.len(),
        });
        tracing::trace!(kind = "ancillary", "file classified");
        Ok(())
    }
}

fn inspect_audio_extension(
    audio: &super::InspectedAudio,
    path: &Path,
    inspection: &mut SourceInspection,
) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case(audio.format.canonical_extension()) {
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::ExtensionMismatch,
            Some(audio.relative_path.clone()),
            format!(
                "{} content will use the canonical .{} extension",
                audio.format,
                audio.format.canonical_extension()
            ),
        ));
    }
}

fn relative(path: &Path, root: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().map(Path::to_owned)
}

fn probable_audio_extension(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "flac"
            | "mp3"
            | "mp2"
            | "mp1"
            | "m4a"
            | "m4b"
            | "mp4"
            | "ogg"
            | "opus"
            | "aac"
            | "ape"
            | "aif"
            | "aiff"
            | "wav"
            | "wv"
            | "wma"
            | "alac"
    )
}

fn extension_matches_artwork(extension: &str, format: super::ArtworkFormat) -> bool {
    match format {
        super::ArtworkFormat::Jpeg => {
            extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
        }
        _ => extension.eq_ignore_ascii_case(format.canonical_extension()),
    }
}

#[cfg(test)]
mod tests;
