use std::fmt;
use std::path::{Path, PathBuf};

use crate::domain::{ArtistCredit, CandidateRelease, InspectedTrack, ReleaseKind, ReleaseTrack};
use crate::guided_matching::{ArtworkSelection, GuidedMatchResult};
use crate::layout::{LayoutError, LayoutPolicy, LayoutTrack, ReleaseLayout, StandaloneLayout};
use crate::matching::RankedCandidate;
use crate::matching_ui::MetadataSelection;
use crate::plan::{
    ArtworkChoice, ArtworkOrigin, GroomingPlan, MetadataBasis, PlanWarning, TagChange, TagField,
    TrackPlan,
};
use crate::provider::source_inspection;
use crate::source::{AudioTags, PlannedTags, SourceInspection};

#[derive(Debug)]
pub enum PlanningError {
    Cancelled,
    Missing(&'static str),
    InvalidMapping(usize),
    Layout(LayoutError),
}

impl fmt::Display for PlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("metadata selection was cancelled"),
            Self::Missing(field) => write!(formatter, "selected metadata has no {field}"),
            Self::InvalidMapping(index) => {
                write!(
                    formatter,
                    "selected metadata has an invalid track mapping at {index}"
                )
            }
            Self::Layout(error) => write!(formatter, "cannot plan destination layout: {error}"),
        }
    }
}

impl std::error::Error for PlanningError {}

impl From<LayoutError> for PlanningError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

pub fn build_plan(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    destination_root: &Path,
) -> Result<GroomingPlan, PlanningError> {
    let (inspection, _) = source_inspection(source);
    let (metadata, reasons, layout, tracks) = match &matched.metadata {
        MetadataSelection::Provider(selected) => {
            provider_plan(source, &inspection.tracks, selected, destination_root)?
        }
        MetadataSelection::ExistingTags => {
            existing_plan(source, &inspection.tracks, destination_root)?
        }
        MetadataSelection::Cancelled => return Err(PlanningError::Cancelled),
    };
    let (artwork, archive_artwork_bytes) = artwork_plan(source, matched);

    Ok(GroomingPlan {
        source_label: source.source.display().to_string(),
        metadata,
        match_selection: matched.match_selection,
        match_reasons: reasons,
        destination_root: destination_root.to_owned(),
        destination: destination_root.join(&layout.directory),
        tracks,
        artwork,
        artwork_alternatives: Vec::new(),
        warnings: matched
            .warnings
            .iter()
            .map(|warning| PlanWarning {
                summary: warning.clone(),
                detail: warning.clone(),
            })
            .collect(),
        preserved_embedded_artwork: source
            .audio
            .iter()
            .filter(|audio| audio.tags.embedded_pictures > 0)
            .count(),
        archive_artwork_bytes,
    })
}

type BuiltPlan = (
    MetadataBasis,
    Vec<String>,
    crate::layout::PlannedLayout,
    Vec<TrackPlan>,
);

fn provider_plan(
    source: &SourceInspection,
    inspected: &[InspectedTrack],
    selected: &RankedCandidate,
    destination_root: &Path,
) -> Result<BuiltPlan, PlanningError> {
    let candidate = &selected.candidate;
    let disc_total = candidate
        .tracks
        .iter()
        .map(|track| track.position.disc)
        .max()
        .unwrap_or(1);
    let mut mappings = selected.mappings.clone();
    mappings.sort_by_key(|mapping| mapping.source_index);
    let mut layout_tracks = Vec::with_capacity(mappings.len());
    for mapping in &mappings {
        let audio = source
            .audio
            .get(mapping.source_index)
            .ok_or(PlanningError::InvalidMapping(mapping.source_index))?;
        let target = candidate
            .tracks
            .get(mapping.candidate_index)
            .ok_or(PlanningError::InvalidMapping(mapping.candidate_index))?;
        layout_tracks.push(LayoutTrack {
            title: target.title.clone(),
            disc: target.position.disc,
            track: target.position.track,
            extension: audio.format.canonical_extension().to_owned(),
        });
    }
    let layout = LayoutPolicy.release(&ReleaseLayout {
        album_artist: candidate.album_artist.clone(),
        title: candidate.title.clone(),
        original_year: candidate.original_year,
        disc_count: disc_total,
        tracks: layout_tracks,
    })?;
    let tracks = mappings
        .iter()
        .zip(&layout.tracks)
        .map(|(mapping, relative_destination)| {
            let audio = source
                .audio
                .get(mapping.source_index)
                .ok_or(PlanningError::InvalidMapping(mapping.source_index))?;
            let inspected = inspected
                .get(mapping.source_index)
                .ok_or(PlanningError::InvalidMapping(mapping.source_index))?;
            let target = candidate
                .tracks
                .get(mapping.candidate_index)
                .ok_or(PlanningError::InvalidMapping(mapping.candidate_index))?;
            let tags = planned_tags(candidate, target);
            Ok(TrackPlan {
                source_relative: audio.relative_path.clone(),
                destination: destination_root.join(relative_destination),
                tag_changes: changes_for(inspected, &audio.tags, candidate, target, &tags),
                planned_tags: Some(tags),
            })
        })
        .collect::<Result<Vec<_>, PlanningError>>()?;
    Ok((
        MetadataBasis::MusicBrainz(candidate.clone()),
        selected
            .reasons
            .iter()
            .map(|reason| reason.summary.clone())
            .collect(),
        layout,
        tracks,
    ))
}

fn existing_plan(
    source: &SourceInspection,
    inspected: &[InspectedTrack],
    destination_root: &Path,
) -> Result<BuiltPlan, PlanningError> {
    if inspected.len() == 1 && inspected[0].album.is_none() {
        let track = &inspected[0];
        let artist = required(&track.artist, "track artist")?;
        let title = required(&track.title, "track title")?;
        let layout = LayoutPolicy.standalone(&StandaloneLayout {
            artist: ArtistCredit::single(artist),
            title: title.clone(),
            extension: source.audio[0].format.canonical_extension().to_owned(),
        })?;
        let tracks = vec![TrackPlan {
            source_relative: source.audio[0].relative_path.clone(),
            destination: destination_root.join(&layout.tracks[0]),
            tag_changes: Vec::new(),
            planned_tags: None,
        }];
        return Ok((
            MetadataBasis::ExistingTags,
            vec!["existing artist and title are internally coherent".into()],
            layout,
            tracks,
        ));
    }

    let first = inspected.first().ok_or(PlanningError::Missing("tracks"))?;
    let album_artist = required(&first.album_artist, "album artist")?;
    let title = required(&first.album, "album title")?;
    let disc_total = inspected
        .iter()
        .filter_map(|track| track.position.map(|position| position.disc))
        .max()
        .unwrap_or(1);
    let layout_tracks = inspected
        .iter()
        .zip(&source.audio)
        .map(|(track, audio)| {
            let position = track
                .position
                .ok_or(PlanningError::Missing("track position"))?;
            Ok(LayoutTrack {
                title: required(&track.title, "track title")?,
                disc: position.disc,
                track: position.track,
                extension: audio.format.canonical_extension().to_owned(),
            })
        })
        .collect::<Result<Vec<_>, PlanningError>>()?;
    let layout = LayoutPolicy.release(&ReleaseLayout {
        album_artist: ArtistCredit::single(album_artist),
        title,
        original_year: first.original_year,
        disc_count: disc_total,
        tracks: layout_tracks,
    })?;
    let tracks = source
        .audio
        .iter()
        .zip(&layout.tracks)
        .map(|(audio, relative_destination)| TrackPlan {
            source_relative: audio.relative_path.clone(),
            destination: destination_root.join(relative_destination),
            tag_changes: Vec::new(),
            planned_tags: None,
        })
        .collect();
    Ok((
        MetadataBasis::ExistingTags,
        vec!["existing album metadata is internally coherent".into()],
        layout,
        tracks,
    ))
}

fn required(value: &Option<String>, field: &'static str) -> Result<String, PlanningError> {
    value
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or(PlanningError::Missing(field))
}

fn planned_tags(release: &CandidateRelease, track: &ReleaseTrack) -> PlannedTags {
    let artist_ids = confident_ids(&track.artist_credit);
    let album_artist_ids = confident_ids(&release.album_artist);
    let track_total = release
        .tracks
        .iter()
        .filter(|other| other.position.disc == track.position.disc)
        .count() as u32;
    let disc_total = release
        .tracks
        .iter()
        .map(|other| other.position.disc)
        .max()
        .unwrap_or(1) as u32;
    PlannedTags {
        title: track.title.clone(),
        artist: track.artist_credit.display.clone(),
        artists: artist_names(&track.artist_credit),
        album: release.title.clone(),
        album_artist: release.album_artist.display.clone(),
        album_artists: artist_names(&release.album_artist),
        artist_ids,
        album_artist_ids,
        compilation: release.kind == ReleaseKind::Compilation,
        original_year: release.original_year,
        track: u32::from(track.position.track),
        track_total,
        disc: u32::from(track.position.disc),
        disc_total,
        recording_id: track.recording_id.clone(),
        release_group_id: release.release_group_id.clone(),
    }
}

fn artist_names(credit: &ArtistCredit) -> Vec<String> {
    if credit.artists.is_empty() {
        vec![credit.display.clone()]
    } else {
        credit
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect()
    }
}

fn confident_ids(credit: &ArtistCredit) -> Option<Vec<String>> {
    (!credit.artists.is_empty())
        .then(|| {
            credit
                .artists
                .iter()
                .map(|artist| artist.musicbrainz_id.clone())
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
}

fn changes_for(
    source: &InspectedTrack,
    source_tags: &AudioTags,
    release: &CandidateRelease,
    track: &ReleaseTrack,
    planned: &PlannedTags,
) -> Vec<TagChange> {
    let mut changes = Vec::new();
    add_change(
        &mut changes,
        TagField::Artist,
        source.artist.clone(),
        planned.artist.clone(),
    );
    add_list_change(
        &mut changes,
        TagField::Artists,
        &source_tags.artists,
        &planned.artists,
    );
    add_change(
        &mut changes,
        TagField::AlbumArtist,
        source.album_artist.clone(),
        planned.album_artist.clone(),
    );
    add_list_change(
        &mut changes,
        TagField::AlbumArtists,
        &source_tags.album_artists,
        &planned.album_artists,
    );
    add_change(
        &mut changes,
        TagField::Album,
        source.album.clone(),
        release.title.clone(),
    );
    add_change(
        &mut changes,
        TagField::Compilation,
        source.compilation.map(yes_no),
        yes_no(planned.compilation),
    );
    if let Some(year) = planned.original_year {
        add_change(
            &mut changes,
            TagField::OriginalYear,
            source_tags.date.clone(),
            year.to_string(),
        );
    }
    add_number_changes(&mut changes, source_tags, planned);
    add_change(
        &mut changes,
        TagField::Title,
        source.title.clone(),
        track.title.clone(),
    );
    add_optional_list_change(
        &mut changes,
        TagField::ArtistIds,
        &source.artist_ids,
        planned.artist_ids.as_ref(),
    );
    add_optional_list_change(
        &mut changes,
        TagField::AlbumArtistIds,
        &source.album_artist_ids,
        planned.album_artist_ids.as_ref(),
    );
    if let Some(recording_id) = &planned.recording_id {
        add_change(
            &mut changes,
            TagField::MusicBrainzRecordingId,
            source.recording_id.clone(),
            recording_id.clone(),
        );
    }
    if let Some(release_group_id) = &planned.release_group_id {
        add_change(
            &mut changes,
            TagField::MusicBrainzReleaseGroupId,
            source.release_group_id.clone(),
            release_group_id.clone(),
        );
    }
    changes
}

fn add_number_changes(changes: &mut Vec<TagChange>, source: &AudioTags, planned: &PlannedTags) {
    for (field, before, after) in [
        (TagField::DiscNumber, source.disc, planned.disc),
        (TagField::DiscTotal, source.disc_total, planned.disc_total),
        (TagField::TrackNumber, source.track, planned.track),
        (
            TagField::TrackTotal,
            source.track_total,
            planned.track_total,
        ),
    ] {
        add_change(
            changes,
            field,
            before.map(|value| value.to_string()),
            after.to_string(),
        );
    }
}

fn add_change(
    changes: &mut Vec<TagChange>,
    field: TagField,
    before: Option<String>,
    after: String,
) {
    if before.as_deref() != Some(after.as_str()) {
        changes.push(TagChange {
            field,
            before,
            after,
        });
    }
}

fn add_list_change(
    changes: &mut Vec<TagChange>,
    field: TagField,
    before: &[String],
    after: &[String],
) {
    if before != after {
        changes.push(TagChange {
            field,
            before: (!before.is_empty()).then(|| before.join("; ")),
            after: after.join("; "),
        });
    }
}

fn add_optional_list_change(
    changes: &mut Vec<TagChange>,
    field: TagField,
    before: &[String],
    after: Option<&Vec<String>>,
) {
    if let Some(after) = after {
        add_list_change(changes, field, before, after);
    }
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

fn artwork_plan(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
) -> (ArtworkChoice, Option<Vec<u8>>) {
    match &matched.artwork {
        ArtworkSelection::Source => {
            let candidate = source
                .selected_artwork
                .and_then(|index| source.artwork.get(index));
            let choice = candidate.map_or_else(no_artwork, |candidate| ArtworkChoice {
                origin: ArtworkOrigin::SourceSidecar {
                    source_name: candidate.relative_path.display().to_string(),
                },
                label: format!("Source {}", candidate.relative_path.display()),
                dimensions: Some(candidate.dimensions),
                output_name: Some(format!("cover.{}", candidate.format.canonical_extension())),
            });
            (choice, None)
        }
        ArtworkSelection::CoverArtArchive(artwork) => {
            let release_group_id = match &matched.metadata {
                MetadataSelection::Provider(selected) => selected
                    .candidate
                    .release_group_id
                    .clone()
                    .unwrap_or_default(),
                MetadataSelection::ExistingTags | MetadataSelection::Cancelled => String::new(),
            };
            (
                ArtworkChoice {
                    origin: ArtworkOrigin::CoverArtArchive { release_group_id },
                    label: "Cover Art Archive front".into(),
                    dimensions: Some(artwork.dimensions),
                    output_name: Some(format!("cover.{}", artwork.format.canonical_extension())),
                },
                Some(artwork.bytes.clone()),
            )
        }
        ArtworkSelection::None => (no_artwork(), None),
    }
}

fn no_artwork() -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::None,
        label: "No sidecar artwork".into(),
        dimensions: None,
        output_name: None,
    }
}

pub fn source_root(source: &SourceInspection) -> PathBuf {
    if source.source.is_dir() {
        source.source.clone()
    } else {
        source
            .source
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_owned()
    }
}

#[cfg(test)]
mod tests;
