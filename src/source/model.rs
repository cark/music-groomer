use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::domain::SourceKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioFormat {
    Flac,
    Mp3,
    M4aAac,
    M4aAlac,
    OggVorbis,
    Opus,
}

impl AudioFormat {
    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::M4aAac | Self::M4aAlac => "m4a",
            Self::OggVorbis => "ogg",
            Self::Opus => "opus",
        }
    }
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
            Self::M4aAac => "AAC in M4A",
            Self::M4aAlac => "ALAC in M4A",
            Self::OggVorbis => "Ogg Vorbis",
            Self::Opus => "Opus",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioProperties {
    pub duration: Duration,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    pub audio_bitrate: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub album_artists: Vec<String>,
    pub date: Option<String>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub recording_id: Option<String>,
    pub release_group_id: Option<String>,
    pub embedded_pictures: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectedAudio {
    pub relative_path: PathBuf,
    pub format: AudioFormat,
    pub properties: AudioProperties,
    pub tags: AudioTags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AncillaryFile {
    pub relative_path: PathBuf,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtworkFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
}

impl ArtworkFormat {
    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Gif => "gif",
        }
    }
}

impl fmt::Display for ArtworkFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::WebP => "WebP",
            Self::Gif => "GIF",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkCandidate {
    pub relative_path: PathBuf,
    pub format: ArtworkFormat,
    pub dimensions: (u32, u32),
    pub name_priority: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSeverity {
    Warning,
    Blocker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    SymlinkSkipped,
    SpecialFile,
    Unreadable,
    UnsupportedAudio,
    CorruptAudio,
    ExtensionMismatch,
    MixedAudioFormats,
    MultipleReleases,
    MissingMetadata,
    ContradictoryMetadata,
    ArtworkExtensionMismatch,
    UnsupportedImage,
    ArtworkChoiceRequired,
    StaleReference,
    CueImage,
    NoAudio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectionNotice {
    pub severity: NoticeSeverity,
    pub kind: NoticeKind,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl InspectionNotice {
    pub fn warning(kind: NoticeKind, path: Option<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            severity: NoticeSeverity::Warning,
            kind,
            path,
            message: message.into(),
        }
    }

    pub fn blocker(kind: NoticeKind, path: Option<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            severity: NoticeSeverity::Blocker,
            kind,
            path,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInspection {
    pub source: PathBuf,
    pub kind: SourceKind,
    pub audio: Vec<InspectedAudio>,
    pub ancillary: Vec<AncillaryFile>,
    pub artwork: Vec<ArtworkCandidate>,
    pub selected_artwork: Option<usize>,
    pub notices: Vec<InspectionNotice>,
}

impl SourceInspection {
    pub fn is_blocked(&self) -> bool {
        self.notices
            .iter()
            .any(|notice| notice.severity == NoticeSeverity::Blocker)
    }

    pub fn duration(&self) -> Duration {
        self.audio
            .iter()
            .map(|audio| audio.properties.duration)
            .sum()
    }
}
