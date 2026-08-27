use std::fmt;
use std::path::PathBuf;

use crate::domain::CandidateRelease;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagField {
    Artist,
    ArtistIds,
    AlbumArtist,
    AlbumArtistIds,
    Album,
    Compilation,
    OriginalYear,
    DiscNumber,
    TrackNumber,
    Title,
    MusicBrainzRecordingId,
    MusicBrainzReleaseGroupId,
}

impl fmt::Display for TagField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Artist => "artist",
            Self::ArtistIds => "MusicBrainz artist IDs",
            Self::AlbumArtist => "album artist",
            Self::AlbumArtistIds => "MusicBrainz album-artist IDs",
            Self::Album => "album",
            Self::Compilation => "compilation",
            Self::OriginalYear => "original year",
            Self::DiscNumber => "disc number",
            Self::TrackNumber => "track number",
            Self::Title => "title",
            Self::MusicBrainzRecordingId => "MusicBrainz recording ID",
            Self::MusicBrainzReleaseGroupId => "MusicBrainz release-group ID",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagChange {
    pub field: TagField,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackPlan {
    pub source_name: String,
    pub destination: PathBuf,
    pub tag_changes: Vec<TagChange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkOrigin {
    SourceSidecar { source_name: String },
    CoverArtArchive { release_group_id: String },
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkChoice {
    pub origin: ArtworkOrigin,
    pub label: String,
    pub dimensions: Option<(u32, u32)>,
    pub output_name: Option<String>,
}

impl ArtworkChoice {
    pub fn description(&self) -> String {
        match self.dimensions {
            Some((width, height)) => format!("{} ({width}x{height})", self.label),
            None => self.label.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanWarning {
    pub summary: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataBasis {
    MusicBrainz(CandidateRelease),
    ExistingTags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchSelection {
    Automatic,
    UserChosen,
    ExistingTags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroomingPlan {
    pub source_label: String,
    pub metadata: MetadataBasis,
    pub match_selection: MatchSelection,
    pub match_reasons: Vec<String>,
    pub destination_root: PathBuf,
    pub destination: PathBuf,
    pub tracks: Vec<TrackPlan>,
    pub artwork: ArtworkChoice,
    pub artwork_alternatives: Vec<ArtworkChoice>,
    pub warnings: Vec<PlanWarning>,
    pub preserved_embedded_artwork: usize,
}

impl GroomingPlan {
    pub fn tag_change_count(&self) -> usize {
        self.tracks
            .iter()
            .map(|track| track.tag_changes.len())
            .sum()
    }

    pub fn filename_change_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|track| {
                track.destination.file_name().and_then(|name| name.to_str())
                    != Some(track.source_name.as_str())
            })
            .count()
    }

    pub fn with_artwork(mut self, artwork: ArtworkChoice) -> Self {
        if self.artwork != artwork {
            let previous = std::mem::replace(&mut self.artwork, artwork.clone());
            self.artwork_alternatives
                .retain(|alternative| alternative != &artwork);
            self.artwork_alternatives.push(previous);
        }
        self
    }

    pub fn with_destination_root(mut self, destination_root: PathBuf) -> Result<Self, PlanError> {
        let relative_destination = self
            .destination
            .strip_prefix(&self.destination_root)
            .map_err(|_| PlanError::DestinationOutsideRoot(self.destination.clone()))?
            .to_owned();
        let relative_tracks: Result<Vec<_>, _> = self
            .tracks
            .iter()
            .map(|track| {
                track
                    .destination
                    .strip_prefix(&self.destination_root)
                    .map(PathBuf::from)
                    .map_err(|_| PlanError::DestinationOutsideRoot(track.destination.clone()))
            })
            .collect();

        self.destination = destination_root.join(relative_destination);
        for (track, relative) in self.tracks.iter_mut().zip(relative_tracks?) {
            track.destination = destination_root.join(relative);
        }
        self.destination_root = destination_root;
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    DestinationOutsideRoot(PathBuf),
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DestinationOutsideRoot(path) => write!(
                formatter,
                "planned destination {} is outside its destination root",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PlanError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyReport {
    pub destination: PathBuf,
    pub tracks_validated: usize,
    pub artwork_validated: bool,
    pub source_unchanged: bool,
    pub simulated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtistCredit, ReleaseKind};

    fn plan() -> GroomingPlan {
        GroomingPlan {
            source_label: "incoming".into(),
            metadata: MetadataBasis::MusicBrainz(CandidateRelease {
                provider_key: "candidate".into(),
                title: "Album".into(),
                album_artist: ArtistCredit::single("Artist"),
                original_year: 2000,
                kind: ReleaseKind::Album,
                tracks: Vec::new(),
                release_group_id: None,
                exact_release_id: None,
            }),
            match_selection: MatchSelection::Automatic,
            match_reasons: Vec::new(),
            destination_root: PathBuf::new(),
            destination: "Artist/2000 - Album".into(),
            tracks: vec![TrackPlan {
                source_name: "old.flac".into(),
                destination: "Artist/2000 - Album/01 - New.flac".into(),
                tag_changes: vec![TagChange {
                    field: TagField::Title,
                    before: Some("Old".into()),
                    after: "New".into(),
                }],
            }],
            artwork: ArtworkChoice {
                origin: ArtworkOrigin::None,
                label: "No sidecar artwork".into(),
                dimensions: None,
                output_name: None,
            },
            artwork_alternatives: Vec::new(),
            warnings: Vec::new(),
            preserved_embedded_artwork: 1,
        }
    }

    #[test]
    fn summarizes_changes_without_hiding_the_underlying_plan() {
        let plan = plan();

        assert_eq!(plan.tag_change_count(), 1);
        assert_eq!(plan.filename_change_count(), 1);
        assert_eq!(plan.tracks[0].tag_changes[0].field, TagField::Title);
    }

    #[test]
    fn relocates_the_complete_plan_without_changing_relative_layout() {
        let relocated = plan()
            .with_destination_root("/media/music".into())
            .expect("plan paths begin under their root");

        assert_eq!(
            relocated.destination,
            PathBuf::from("/media/music/Artist/2000 - Album")
        );
        assert_eq!(
            relocated.tracks[0].destination,
            PathBuf::from("/media/music/Artist/2000 - Album/01 - New.flac")
        );
    }
}
