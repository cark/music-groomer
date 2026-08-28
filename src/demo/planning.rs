use std::path::Path;

use crate::domain::{ArtistCredit, CandidateRelease, InspectedTrack, ReleaseTrack, SourceKind};
use crate::layout::{LayoutPolicy, LayoutTrack, ReleaseLayout, StandaloneLayout};
use crate::matching::RankedCandidate;
use crate::plan::{
    ArtworkChoice, ArtworkOrigin, GroomingPlan, MatchSelection, MetadataBasis, TagChange, TagField,
    TrackPlan,
};

use super::DemoError;
use super::fixtures::DemoData;

pub(super) fn coherent_standalone(data: &DemoData) -> bool {
    let inspection = &data.inspection;
    inspection.kind == SourceKind::LooseFile
        && inspection.tracks.len() == 1
        && inspection.tracks[0]
            .artist
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && inspection.tracks[0]
            .title
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn build_plan(
    data: &DemoData,
    selected: Option<Box<RankedCandidate>>,
    match_selection: MatchSelection,
    destination_root: &Path,
) -> Result<GroomingPlan, DemoError> {
    let (metadata, reasons, relative_layout, track_changes) = match selected {
        Some(selected) => {
            let candidate = selected.candidate.clone();
            let disc_count = candidate
                .tracks
                .iter()
                .map(|track| track.position.disc)
                .max()
                .unwrap_or(1);
            let tracks = selected
                .mappings
                .iter()
                .map(|mapping| {
                    let target = &candidate.tracks[mapping.candidate_index];
                    LayoutTrack {
                        title: target.title.clone(),
                        disc: target.position.disc,
                        track: target.position.track,
                        extension: data.extensions[mapping.source_index].clone(),
                    }
                })
                .collect();
            let layout = LayoutPolicy
                .release(&ReleaseLayout {
                    album_artist: candidate.album_artist.clone(),
                    title: candidate.title.clone(),
                    original_year: candidate.original_year,
                    disc_count,
                    tracks,
                })
                .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
            let changes = selected
                .mappings
                .iter()
                .map(|mapping| {
                    changes_for(
                        &data.inspection.tracks[mapping.source_index],
                        &candidate,
                        &candidate.tracks[mapping.candidate_index],
                    )
                })
                .collect();
            (
                MetadataBasis::MusicBrainz(candidate),
                selected
                    .reasons
                    .iter()
                    .map(|reason| reason.summary.clone())
                    .collect(),
                layout,
                changes,
            )
        }
        None => {
            let source = &data.inspection.tracks[0];
            let artist = source.artist.as_ref().ok_or_else(|| {
                DemoError::InvalidDemoData("standalone track has no artist".into())
            })?;
            let title = source.title.as_ref().ok_or_else(|| {
                DemoError::InvalidDemoData("standalone track has no title".into())
            })?;
            let layout = LayoutPolicy
                .standalone(&StandaloneLayout {
                    artist: ArtistCredit::single(artist),
                    title: title.clone(),
                    extension: data.extensions[0].clone(),
                })
                .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
            (
                MetadataBasis::ExistingTags,
                vec!["existing artist and title are internally coherent".into()],
                layout,
                vec![Vec::new()],
            )
        }
    };

    let destination = destination_root.join(&relative_layout.directory);
    let tracks = data
        .inspection
        .tracks
        .iter()
        .zip(relative_layout.tracks)
        .zip(track_changes)
        .map(|((source, relative_destination), tag_changes)| TrackPlan {
            source_relative: source.source_name.clone().into(),
            destination: destination_root.join(relative_destination),
            tag_changes,
            planned_tags: None,
        })
        .collect();
    let artwork = data
        .source_artwork
        .clone()
        .or_else(|| data.provider_artwork.clone())
        .unwrap_or_else(no_artwork);
    let artwork_alternatives = if data.source_artwork.is_some() {
        data.provider_artwork.clone().into_iter().collect()
    } else {
        Vec::new()
    };

    Ok(GroomingPlan {
        source_label: data.inspection.source_label.clone(),
        metadata,
        match_selection,
        match_reasons: reasons,
        destination_root: destination_root.to_owned(),
        destination,
        tracks,
        ancillary: Vec::new(),
        ancillary_directories: Vec::new(),
        artwork,
        artwork_alternatives,
        warnings: data.warning.clone().into_iter().collect(),
        preserved_embedded_artwork: data.embedded_artwork_count,
        archive_artwork_bytes: None,
    })
}

fn changes_for(
    source: &InspectedTrack,
    release: &CandidateRelease,
    track: &ReleaseTrack,
) -> Vec<TagChange> {
    let original_year = source.original_year.map(|year| year.to_string());
    let disc_number = source.position.map(|position| position.disc.to_string());
    let track_number = source.position.map(|position| position.track.to_string());
    let artist_ids = confident_ids(&track.artist_credit);
    let album_artist_ids = confident_ids(&release.album_artist);
    let compilation = release.kind == crate::domain::ReleaseKind::Compilation;
    let proposed = vec![
        (
            TagField::Artist,
            source.artist.clone(),
            Some(track.artist_credit.display.clone()),
        ),
        (
            TagField::AlbumArtist,
            source.album_artist.clone(),
            Some(release.album_artist.display.clone()),
        ),
        (
            TagField::Album,
            source.album.clone(),
            Some(release.title.clone()),
        ),
        (
            TagField::OriginalYear,
            original_year,
            release.original_year.map(|year| year.to_string()),
        ),
        (
            TagField::DiscNumber,
            disc_number,
            Some(track.position.disc.to_string()),
        ),
        (
            TagField::TrackNumber,
            track_number,
            Some(track.position.track.to_string()),
        ),
        (
            TagField::Title,
            source.title.clone(),
            Some(track.title.clone()),
        ),
    ];

    let mut changes: Vec<_> = proposed
        .into_iter()
        .filter_map(|(field, before, after)| {
            let after = after?;
            (before.as_deref() != Some(after.as_str())).then_some(TagChange {
                field,
                before,
                after,
            })
        })
        .collect();

    add_confident_list_change(
        &mut changes,
        TagField::ArtistIds,
        &source.artist_ids,
        artist_ids,
    );
    add_confident_list_change(
        &mut changes,
        TagField::AlbumArtistIds,
        &source.album_artist_ids,
        album_artist_ids,
    );
    if source.compilation != Some(compilation) {
        changes.push(TagChange {
            field: TagField::Compilation,
            before: source.compilation.map(yes_no),
            after: yes_no(compilation),
        });
    }

    if source.recording_id != track.recording_id
        && let Some(recording_id) = &track.recording_id
    {
        changes.push(TagChange {
            field: TagField::MusicBrainzRecordingId,
            before: source.recording_id.clone(),
            after: recording_id.clone(),
        });
    }
    if source.release_group_id != release.release_group_id
        && let Some(release_group_id) = &release.release_group_id
    {
        changes.push(TagChange {
            field: TagField::MusicBrainzReleaseGroupId,
            before: source.release_group_id.clone(),
            after: release_group_id.clone(),
        });
    }

    changes
}

fn confident_ids(credit: &crate::domain::ArtistCredit) -> Option<Vec<String>> {
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

fn add_confident_list_change(
    changes: &mut Vec<TagChange>,
    field: TagField,
    before: &[String],
    after: Option<Vec<String>>,
) {
    let Some(after) = after else {
        return;
    };
    if before != after {
        changes.push(TagChange {
            field,
            before: (!before.is_empty()).then(|| before.join("; ")),
            after: after.join("; "),
        });
    }
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

fn no_artwork() -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::None,
        label: "No sidecar artwork".into(),
        dimensions: None,
        output_name: None,
    }
}
