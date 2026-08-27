use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::{CandidateRelease, InspectedTrack, Inspection, Position, SourceKind};
use crate::source::{AudioTags, SourceInspection};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSearch {
    pub kind: SourceKind,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub artist_ids: Vec<String>,
    pub album_artist_ids: Vec<String>,
    pub title: Option<String>,
    pub release_group_id: Option<String>,
    pub recording_ids: Vec<String>,
    pub track_count: usize,
}

impl ProviderSearch {
    pub fn is_usable(&self) -> bool {
        self.release_group_id.is_some()
            || !self.recording_ids.is_empty()
            || (self.artist.is_some() && (self.album.is_some() || self.title.is_some()))
    }
}

pub trait MetadataProvider {
    fn search(
        &mut self,
        search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderSearchResult {
    pub candidates: Vec<CandidateRelease>,
    pub warnings: Vec<String>,
}

pub trait ProviderProgress {
    fn event(&mut self, event: ProviderEvent) -> Result<(), ProviderError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEvent {
    Requesting(&'static str),
    Waiting { seconds: u64, reason: WaitReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    RateLimit,
    Retry,
}

impl ProviderProgress for () {
    fn event(&mut self, _event: ProviderEvent) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[derive(Debug)]
pub enum ProviderError {
    InsufficientEvidence,
    Network(String),
    HttpStatus {
        operation: &'static str,
        status: u16,
    },
    InvalidResponse(String),
    Progress(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientEvidence => {
                formatter.write_str("the source has too little metadata to search MusicBrainz")
            }
            Self::Network(message) => write!(formatter, "provider request failed: {message}"),
            Self::HttpStatus { operation, status } => {
                write!(formatter, "{operation} returned HTTP {status}")
            }
            Self::InvalidResponse(message) => {
                write!(formatter, "provider returned invalid data: {message}")
            }
            Self::Progress(message) => {
                write!(formatter, "cannot report provider progress: {message}")
            }
        }
    }
}

impl std::error::Error for ProviderError {}

pub fn source_inspection(source: &SourceInspection) -> (Inspection, ProviderSearch) {
    let tracks = source
        .audio
        .iter()
        .map(|audio| InspectedTrack {
            source_name: audio.relative_path.display().to_string(),
            title: audio.tags.title.clone(),
            artist: audio.tags.artist.clone(),
            album: audio.tags.album.clone(),
            album_artist: audio.tags.album_artist.clone(),
            artist_ids: audio.tags.artist_ids.clone(),
            album_artist_ids: audio.tags.album_artist_ids.clone(),
            compilation: audio.tags.compilation,
            original_year: original_year(&audio.tags),
            position: position(&audio.tags),
            duration_ms: u64::try_from(audio.properties.duration.as_millis()).unwrap_or(u64::MAX),
            recording_id: audio.tags.recording_id.clone(),
            release_group_id: audio.tags.release_group_id.clone(),
        })
        .collect::<Vec<_>>();
    let inspection = Inspection {
        source_label: source.source.display().to_string(),
        kind: source.kind,
        tracks,
    };
    let search = ProviderSearch {
        kind: source.kind,
        album: common(&inspection, |track| track.album.as_deref()),
        artist: common(&inspection, |track| {
            track.album_artist.as_deref().or(track.artist.as_deref())
        }),
        artist_ids: common_ids(&inspection, |track| &track.artist_ids),
        album_artist_ids: common_ids(&inspection, |track| &track.album_artist_ids),
        title: (source.kind == SourceKind::LooseFile)
            .then(|| inspection.tracks.first()?.title.clone())
            .flatten(),
        release_group_id: common(&inspection, |track| track.release_group_id.as_deref()),
        recording_ids: inspection
            .tracks
            .iter()
            .filter_map(|track| track.recording_id.clone())
            .collect(),
        track_count: inspection.tracks.len(),
    };
    (inspection, search)
}

fn common_ids(
    inspection: &Inspection,
    value: impl Fn(&InspectedTrack) -> &[String],
) -> Vec<String> {
    let Some(first) = inspection.tracks.first().map(&value) else {
        return Vec::new();
    };
    if inspection.tracks.iter().all(|track| value(track) == first) {
        first.to_vec()
    } else {
        Vec::new()
    }
}

fn position(tags: &AudioTags) -> Option<Position> {
    let track = u16::try_from(tags.track?).ok()?;
    let disc = u16::try_from(tags.disc.unwrap_or(1)).ok()?;
    (track > 0 && disc > 0).then_some(Position::new(disc, track))
}

fn original_year(tags: &AudioTags) -> Option<u16> {
    tags.date.as_deref()?.trim_start().get(..4)?.parse().ok()
}

fn common(
    inspection: &Inspection,
    value: impl Fn(&InspectedTrack) -> Option<&str>,
) -> Option<String> {
    let first = value(inspection.tracks.first()?)?;
    inspection
        .tracks
        .iter()
        .all(|track| value(track).is_some_and(|value| normalized(value) == normalized(first)))
        .then(|| first.to_owned())
}

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::source::{AudioFormat, AudioProperties, InspectedAudio};

    #[test]
    fn builds_search_and_matching_values_from_real_inspection_shape() {
        let source = SourceInspection {
            source: PathBuf::from("incoming/album"),
            kind: SourceKind::AlbumDirectory,
            audio: vec![audio(1, "1999-04-02"), audio(2, "1999-04-02")],
            ancillary: Vec::new(),
            artwork: Vec::new(),
            selected_artwork: None,
            notices: Vec::new(),
        };

        let (inspection, search) = source_inspection(&source);

        assert_eq!(inspection.tracks[0].position, Some(Position::new(1, 1)));
        assert_eq!(inspection.tracks[0].original_year, Some(1999));
        assert_eq!(search.album.as_deref(), Some("Album"));
        assert_eq!(search.artist.as_deref(), Some("Album Artist"));
        assert_eq!(search.track_count, 2);
        assert!(search.is_usable());
    }

    #[test]
    fn cosmetic_case_and_spacing_differences_still_form_a_search() {
        let mut source = SourceInspection {
            source: PathBuf::from("incoming/album"),
            kind: SourceKind::AlbumDirectory,
            audio: vec![audio(1, "1999"), audio(2, "1999")],
            ancillary: Vec::new(),
            artwork: Vec::new(),
            selected_artwork: None,
            notices: Vec::new(),
        };
        source.audio[1].tags.album = Some("  album  ".into());

        let (_, search) = source_inspection(&source);

        assert_eq!(search.album.as_deref(), Some("Album"));
        assert!(search.is_usable());
    }

    fn audio(track: u32, date: &str) -> InspectedAudio {
        InspectedAudio {
            relative_path: PathBuf::from(format!("{track:02}.flac")),
            format: AudioFormat::Flac,
            properties: AudioProperties {
                duration: Duration::from_secs(120),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                audio_bitrate: None,
            },
            tags: AudioTags {
                title: Some(format!("Track {track}")),
                artist: Some("Track Artist".into()),
                album: Some("Album".into()),
                album_artist: Some("Album Artist".into()),
                date: Some(date.into()),
                track: Some(track),
                disc: Some(1),
                ..AudioTags::default()
            },
        }
    }
}
