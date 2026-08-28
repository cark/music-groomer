use crate::domain::{CandidateRelease, InspectedTrack, ReleaseTrack};
use crate::plan::{TagChange, TagField};
use crate::source::{AudioTags, PlannedTags};

pub(super) fn changes_for(
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
