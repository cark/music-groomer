use std::path::Path;

use crate::domain::{ArtistCredit, CandidateRelease, InspectedTrack, ReleaseKind, ReleaseTrack};
use crate::layout::{LayoutPolicy, LayoutTrack, ReleaseLayout, StandaloneLayout};
use crate::matching::RankedCandidate;
use crate::plan::{MetadataBasis, TrackPlan};
use crate::source::{PlannedTags, SourceInspection};

use super::PlanningError;
use super::changes::changes_for;

pub(super) type BuiltPlan = (
    MetadataBasis,
    Vec<String>,
    crate::layout::PlannedLayout,
    Vec<TrackPlan>,
);

pub(super) fn provider_plan(
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

pub(super) fn existing_plan(
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
    PlannedTags {
        title: track.title.clone(),
        artist: track.artist_credit.display.clone(),
        artists: artist_names(&track.artist_credit),
        album: release.title.clone(),
        album_artist: release.album_artist.display.clone(),
        album_artists: artist_names(&release.album_artist),
        artist_ids: confident_ids(&track.artist_credit),
        album_artist_ids: confident_ids(&release.album_artist),
        compilation: release.kind == ReleaseKind::Compilation,
        original_year: release.original_year,
        track: u32::from(track.position.track),
        track_total: release
            .tracks
            .iter()
            .filter(|other| other.position.disc == track.position.disc)
            .count() as u32,
        disc: u32::from(track.position.disc),
        disc_total: u32::from(
            release
                .tracks
                .iter()
                .map(|other| other.position.disc)
                .max()
                .unwrap_or(1),
        ),
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
