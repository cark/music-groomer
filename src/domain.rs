use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub name: String,
    pub musicbrainz_id: Option<String>,
}

impl Artist {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            musicbrainz_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistCredit {
    pub display: String,
    pub artists: Vec<Artist>,
}

impl ArtistCredit {
    pub fn single(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            display: name.clone(),
            artists: vec![Artist::named(name)],
        }
    }

    pub fn credited(display: impl Into<String>, artists: Vec<Artist>) -> Self {
        Self {
            display: display.into(),
            artists,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    AlbumDirectory,
    LooseFile,
}

impl SourceKind {
    pub fn requires_complete_release(self) -> bool {
        matches!(self, Self::AlbumDirectory)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub disc: u16,
    pub track: u16,
}

impl Position {
    pub const fn new(disc: u16, track: u16) -> Self {
        Self { disc, track }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectedTrack {
    pub source_name: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist_ids: Vec<String>,
    pub album_artist_ids: Vec<String>,
    pub compilation: Option<bool>,
    pub original_year: Option<u16>,
    pub position: Option<Position>,
    pub duration_ms: u64,
    pub recording_id: Option<String>,
    pub release_group_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inspection {
    pub source_label: String,
    pub kind: SourceKind,
    pub tracks: Vec<InspectedTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseKind {
    Album,
    Single,
    Ep,
    Compilation,
    Other(String),
}

impl fmt::Display for ReleaseKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Album => "Album",
            Self::Single => "Single",
            Self::Ep => "EP",
            Self::Compilation => "Compilation",
            Self::Other(primary_type) if primary_type.is_empty() => "Other",
            Self::Other(primary_type) => return write!(formatter, "Other ({primary_type})"),
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseTrack {
    pub title: String,
    pub artist_credit: ArtistCredit,
    pub position: Position,
    pub duration_ms: u64,
    pub recording_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRelease {
    pub provider_key: String,
    pub title: String,
    pub album_artist: ArtistCredit,
    pub original_year: Option<u16>,
    pub kind: ReleaseKind,
    pub tracks: Vec<ReleaseTrack>,
    pub release_group_id: Option<String>,
    pub exact_release_id: Option<String>,
}

impl CandidateRelease {
    pub fn disc_count(&self) -> usize {
        self.tracks
            .iter()
            .map(|track| track.position.disc)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }

    pub fn human_label(&self) -> String {
        format!(
            "{} — {} ({}, {}, {} {}, {} {})",
            self.album_artist.display,
            self.title,
            self.original_year
                .map_or_else(|| "year unknown".to_owned(), |year| year.to_string()),
            self.kind,
            self.disc_count(),
            if self.disc_count() == 1 {
                "disc"
            } else {
                "discs"
            },
            self.tracks.len(),
            if self.tracks.len() == 1 {
                "track"
            } else {
                "tracks"
            }
        )
    }
}
