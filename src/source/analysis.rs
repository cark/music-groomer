use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::domain::SourceKind;

use super::{InspectionNotice, NoticeKind, SourceInspection};

pub(super) fn finish(root: &Path, inspection: &mut SourceInspection) {
    inspect_release_shape(inspection);
    inspect_artwork_choice(inspection);
    inspect_cue_shape(root, inspection);
    inspect_stale_references(inspection);
}

fn inspect_release_shape(inspection: &mut SourceInspection) {
    if inspection.audio.is_empty() {
        inspection.notices.push(InspectionNotice::blocker(
            NoticeKind::NoAudio,
            None,
            "no supported audio files were found",
        ));
        return;
    }
    let formats = inspection
        .audio
        .iter()
        .map(|audio| audio.format)
        .collect::<BTreeSet<_>>();
    if formats.len() > 1 {
        let labels = formats
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::MixedAudioFormats,
            None,
            format!("release contains mixed supported formats: {labels}"),
        ));
    }

    let albums = values(&inspection.audio, |audio| audio.tags.album.as_deref());
    if albums.len() > 1 && inspection.kind == SourceKind::AlbumDirectory {
        let labels = albums.iter().cloned().collect::<Vec<_>>().join(", ");
        let normalized = albums
            .iter()
            .map(|album| normalized_album_title(album))
            .collect::<BTreeSet<_>>();
        if normalized.len() > 1 {
            inspection.notices.push(InspectionNotice::blocker(
                NoticeKind::MultipleReleases,
                None,
                format!("directory appears to contain multiple releases: {labels}"),
            ));
        } else {
            inspection.notices.push(InspectionNotice::warning(
                NoticeKind::ContradictoryMetadata,
                None,
                format!("tracks use cosmetically different album titles: {labels}"),
            ));
        }
    }
    inspect_common_field(inspection, "album artist", |audio| {
        audio.tags.album_artist.as_deref()
    });
    inspect_common_field(inspection, "date", |audio| audio.tags.date.as_deref());
    inspect_positions(inspection);
    inspect_missing_metadata(inspection);
}

fn inspect_positions(inspection: &mut SourceInspection) {
    let mut positions = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let mut track_totals = BTreeSet::new();
    let mut disc_totals = BTreeSet::new();
    for audio in &inspection.audio {
        if let Some(track) = audio.tags.track {
            let position = (audio.tags.disc.unwrap_or(1), track);
            if !positions.insert(position) {
                duplicates.insert(position);
            }
        }
        track_totals.extend(audio.tags.track_total);
        disc_totals.extend(audio.tags.disc_total);
    }
    if !duplicates.is_empty() {
        let positions = duplicates
            .into_iter()
            .map(|(disc, track)| format!("{disc}-{track}"))
            .collect::<Vec<_>>()
            .join(", ");
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::ContradictoryMetadata,
            None,
            format!("duplicate disc-track positions: {positions}"),
        ));
    }
    if track_totals.len() > 1 || disc_totals.len() > 1 {
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::ContradictoryMetadata,
            None,
            "tracks disagree about disc or track totals",
        ));
    }
}

fn inspect_common_field(
    inspection: &mut SourceInspection,
    label: &str,
    value: impl Fn(&super::InspectedAudio) -> Option<&str>,
) {
    let values = values(&inspection.audio, value);
    if values.len() > 1 {
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::ContradictoryMetadata,
            None,
            format!(
                "tracks disagree about {label}: {}",
                values.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
}

fn inspect_missing_metadata(inspection: &mut SourceInspection) {
    let mut missing = BTreeSet::new();
    for audio in &inspection.audio {
        if audio.tags.title.is_none() {
            missing.insert("title");
        }
        if audio.tags.artist.is_none() {
            missing.insert("artist");
        }
        if inspection.kind == SourceKind::AlbumDirectory {
            if audio.tags.album.is_none() {
                missing.insert("album");
            }
            if audio.tags.album_artist.is_none() {
                missing.insert("album artist");
            }
            if audio.tags.track.is_none() {
                missing.insert("track number");
            }
        }
    }
    if !missing.is_empty() {
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::MissingMetadata,
            None,
            format!(
                "some tracks are missing: {}",
                missing.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
}

fn inspect_artwork_choice(inspection: &mut SourceInspection) {
    let Some(best_priority) = inspection
        .artwork
        .iter()
        .map(|artwork| artwork.name_priority)
        .min()
    else {
        return;
    };
    let matching = inspection
        .artwork
        .iter()
        .enumerate()
        .filter(|(_, artwork)| artwork.name_priority == best_priority)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if matching.len() == 1 {
        inspection.selected_artwork = matching.first().copied();
    } else {
        inspection.notices.push(InspectionNotice::warning(
            NoticeKind::ArtworkChoiceRequired,
            None,
            "equally preferred source covers require a later user choice",
        ));
    }
}

fn inspect_cue_shape(root: &Path, inspection: &mut SourceInspection) {
    if inspection.kind != SourceKind::AlbumDirectory || inspection.audio.len() != 1 {
        return;
    }
    let cue_paths = inspection
        .ancillary
        .iter()
        .filter(|ancillary| has_extension(&ancillary.relative_path, "cue"))
        .map(|ancillary| ancillary.relative_path.clone())
        .collect::<Vec<_>>();
    for relative_path in cue_paths {
        let path = root.join(&relative_path);
        let contents = match fs::read(&path) {
            Ok(contents) => contents,
            Err(error) => {
                inspection.notices.push(InspectionNotice::blocker(
                    NoticeKind::Unreadable,
                    Some(relative_path),
                    format!("cannot inspect cue sheet structure: {error}"),
                ));
                continue;
            }
        };
        let track_count = contents
            .split(|byte| *byte == b'\n')
            .filter(|line| starts_with_ascii_keyword(line, b"TRACK "))
            .count();
        if track_count > 1 {
            inspection.notices.push(InspectionNotice::blocker(
                NoticeKind::CueImage,
                Some(relative_path),
                "cue sheet describes multiple virtual tracks in one audio image; split it externally before grooming",
            ));
        }
    }
}

fn normalized_album_title(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn starts_with_ascii_keyword(line: &[u8], keyword: &[u8]) -> bool {
    let line = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |start| &line[start..]);
    line.get(..keyword.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(keyword))
}

fn inspect_stale_references(inspection: &mut SourceInspection) {
    let names_will_change = inspection
        .notices
        .iter()
        .any(|notice| notice.kind == NoticeKind::ExtensionMismatch);
    if !names_will_change {
        return;
    }
    for ancillary in &inspection.ancillary {
        if ["cue", "m3u", "m3u8"]
            .iter()
            .any(|extension| has_extension(&ancillary.relative_path, extension))
        {
            inspection.notices.push(InspectionNotice::warning(
                NoticeKind::StaleReference,
                Some(ancillary.relative_path.clone()),
                "audio extension correction may leave preserved references stale",
            ));
        }
    }
}

fn values(
    audio: &[super::InspectedAudio],
    value: impl Fn(&super::InspectedAudio) -> Option<&str>,
) -> BTreeSet<String> {
    audio.iter().filter_map(value).map(str::to_owned).collect()
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}
