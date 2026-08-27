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
        }
    }
}

impl std::error::Error for InspectionError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceInspector {
    audio: LoftyAudioReader,
}

impl SourceInspector {
    pub fn inspect(&self, source: &Path) -> Result<SourceInspection, InspectionError> {
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
        };
        if kind == SourceKind::LooseFile {
            self.inspect_file(source, root, &mut inspection);
        } else {
            self.inspect_directory(source, root, &mut inspection);
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
        super::analysis::finish(root, &mut inspection);
        Ok(inspection)
    }

    fn inspect_directory(&self, directory: &Path, root: &Path, inspection: &mut SourceInspection) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    relative(directory, root),
                    format!("cannot read directory: {error}"),
                ));
                return;
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
                self.inspect_directory(&path, root, inspection);
            } else if file_type.is_file() {
                self.inspect_file(&path, root, inspection);
            } else {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::SpecialFile,
                    relative_path,
                    "special filesystem object cannot be preserved safely",
                ));
            }
        }
    }

    fn inspect_file(&self, path: &Path, root: &Path, inspection: &mut SourceInspection) {
        let relative_path = relative(path, root).unwrap_or_else(|| path.to_owned());
        let metadata = match File::open(path).and_then(|file| file.metadata()) {
            Ok(metadata) => metadata,
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    Some(relative_path),
                    format!("cannot read file: {error}"),
                ));
                return;
            }
        };
        match self.audio.probe(path) {
            Ok(AudioProbe::Supported(audio)) => {
                let mut audio = *audio;
                audio.relative_path = relative_path.clone();
                inspect_audio_extension(&audio, path, inspection);
                inspection.audio.push(audio);
                return;
            }
            Ok(AudioProbe::Unsupported(format)) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::UnsupportedAudio,
                    Some(relative_path),
                    format!("recognized audio format {format} is not supported in v0.1"),
                ));
                return;
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
                return;
            }
        }

        if probable_audio_extension(path) {
            inspection.notices.push(InspectionNotice::blocker(
                NoticeKind::UnsupportedAudio,
                Some(relative_path),
                "file looks like audio from its name but its contents are unsupported or damaged",
            ));
            return;
        }

        match artwork::probe(path) {
            Ok(ArtworkProbe::Supported { format, dimensions }) => {
                let is_root = relative_path
                    .parent()
                    .is_none_or(|parent| parent.as_os_str().is_empty());
                if is_root && let Some(name_priority) = artwork::name_priority(path) {
                    let actual_extension = path
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default();
                    if !extension_matches_artwork(actual_extension, format) {
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
                        format,
                        dimensions,
                        name_priority,
                    });
                }
            }
            Ok(ArtworkProbe::Unsupported(format)) => {
                inspection.notices.push(InspectionNotice::warning(
                    NoticeKind::UnsupportedImage,
                    Some(relative_path.clone()),
                    format!("{format} cannot be canonical artwork and will be preserved unchanged"),
                ));
            }
            Ok(ArtworkProbe::NotImage) => {}
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
