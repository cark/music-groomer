use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    AlbumDirectory,
    LooseFile,
}

impl SourceKind {
    pub fn requires_complete_release(self) -> bool {
        matches!(self, Self::AlbumDirectory)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub disc: u16,
    pub track: u16,
}

impl Position {
    pub const fn new(disc: u16, track: u16) -> Self {
        Self { disc, track }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub source_label: String,
    pub kind: SourceKind,
    pub tracks: Vec<InspectedTrack>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseKind {
    Album,
    Single,
    Compilation,
}

impl fmt::Display for ReleaseKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Album => "Album",
            Self::Single => "Single",
            Self::Compilation => "Compilation",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseTrack {
    pub title: String,
    pub artist_credit: ArtistCredit,
    pub position: Position,
    pub duration_ms: u64,
    pub recording_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateRelease {
    pub provider_key: String,
    pub title: String,
    pub album_artist: ArtistCredit,
    pub original_year: u16,
    pub kind: ReleaseKind,
    pub tracks: Vec<ReleaseTrack>,
    pub release_group_id: Option<String>,
    pub exact_release_id: Option<String>,
}

impl CandidateRelease {
    pub fn human_label(&self) -> String {
        format!(
            "{} — {} ({}, {}, {} {})",
            self.album_artist.display,
            self.title,
            self.original_year,
            self.kind,
            self.tracks.len(),
            if self.tracks.len() == 1 {
                "track"
            } else {
                "tracks"
            }
        )
    }
}
