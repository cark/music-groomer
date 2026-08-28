use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{Artist, ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack};
use crate::guided_matching::{ArtworkSelection, GuidedMatchResult, MetadataProvenance};
use crate::matching::{MatchDecision, MatchPolicy};
use crate::matching_ui::MetadataSelection;
use crate::plan::{ArtworkOrigin, MatchSelection};
use crate::provider::{ProviderArtwork, source_inspection};
use crate::source::{
    AncillaryFile, ArtworkCandidate, ArtworkFormat, AudioFormat, AudioProperties, AudioTags,
    InspectedAudio, SourceInspection,
};

use super::*;

#[test]
fn loose_track_uses_its_real_release_position_and_total() {
    let source = loose_source();
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::CoverArtArchive(ProviderArtwork {
            bytes: vec![1, 2, 3],
            format: ArtworkFormat::Jpeg,
            dimensions: (600, 600),
        }),
    );

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();
    let tags = plan.tracks[0].planned_tags.as_ref().unwrap();

    assert_eq!(tags.track, 1);
    assert_eq!(tags.track_total, 2);
    assert_eq!(tags.disc, 1);
    assert_eq!(tags.disc_total, 1);
    assert_eq!(
        plan.destination,
        Path::new("/library/Deee-Lite/1990 - Groove Is in the Heart")
    );
    assert_eq!(plan.archive_artwork_bytes, Some(vec![1, 2, 3]));
}

#[test]
fn unmatched_loose_track_does_not_invent_release_tags_or_year() {
    let source = loose_source();
    let matched = result(MetadataSelection::ExistingTags, ArtworkSelection::None);

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert_eq!(
        plan.destination,
        Path::new("/library/Deee-Lite/Standalone Tracks/Groove Is in the Heart")
    );
    assert!(plan.tracks[0].planned_tags.is_none());
    assert!(plan.tracks[0].tag_changes.is_empty());
}

#[test]
fn selected_source_artwork_gets_a_canonical_native_name() {
    let mut source = loose_source();
    source.artwork.push(ArtworkCandidate {
        relative_path: PathBuf::from("cover.jpg"),
        format: ArtworkFormat::Jpeg,
        dimensions: (500, 500),
        name_priority: 0,
    });
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("cover.jpg"),
        bytes: 100,
    });
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("folder.png"),
        bytes: 200,
    });
    source.artwork.push(ArtworkCandidate {
        relative_path: PathBuf::from("folder.png"),
        format: ArtworkFormat::Png,
        dimensions: (1000, 1000),
        name_priority: 2,
    });
    source.selected_artwork = Some(0);
    let matched = result(MetadataSelection::ExistingTags, ArtworkSelection::Source(1));

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert_eq!(plan.artwork.output_name.as_deref(), Some("cover.png"));
    assert!(matches!(
        plan.artwork.origin,
        ArtworkOrigin::SourceSidecar { ref source_name } if source_name == "folder.png"
    ));
    assert!(plan.ancillary.iter().any(|file| {
        file.source_relative == Path::new("cover.jpg")
            && file.destination_relative == Path::new("original-artwork/cover.jpg")
    }));
    assert!(
        plan.ancillary
            .iter()
            .all(|file| file.source_relative != Path::new("folder.png"))
    );
}

#[test]
fn planned_audio_renames_warn_for_each_preserved_reference_file() {
    let mut source = loose_source();
    for name in ["album.cue", "playlist.m3u", "playlist.m3u8"] {
        source.ancillary.push(AncillaryFile {
            relative_path: PathBuf::from(name),
            bytes: 1,
        });
    }
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::None,
    );

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    for name in ["album.cue", "playlist.m3u", "playlist.m3u8"] {
        assert!(plan.warnings.iter().any(|warning| {
            warning.summary.starts_with(name) && warning.summary.contains("references stale")
        }));
    }
}

#[test]
fn planned_reference_warning_does_not_duplicate_an_inspection_warning() {
    let mut source = loose_source();
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("playlist.m3u"),
        bytes: 1,
    });
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let mut matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::None,
    );
    source
        .notices
        .push(crate::source::InspectionNotice::warning(
            crate::source::NoticeKind::StaleReference,
            Some(PathBuf::from("playlist.m3u")),
            "audio extension correction may leave preserved references stale",
        ));
    matched.warnings.push(
        "playlist.m3u: audio extension correction may leave preserved references stale".into(),
    );

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert_eq!(
        plan.warnings
            .iter()
            .filter(|warning| warning.summary.contains("references stale"))
            .count(),
        1
    );
}

#[test]
fn unchanged_audio_paths_do_not_warn_about_preserved_references() {
    let mut source = loose_source();
    source.audio[0].relative_path = PathBuf::from("Groove Is in the Heart.mp3");
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("playlist.m3u"),
        bytes: 1,
    });
    let matched = result(MetadataSelection::ExistingTags, ArtworkSelection::None);

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert!(
        plan.warnings
            .iter()
            .all(|warning| !warning.summary.contains("references stale"))
    );
}

#[test]
fn provider_plan_removes_a_missing_metadata_warning_it_resolves() {
    let mut source = loose_source();
    let notice = crate::source::InspectionNotice::warning(
        crate::source::NoticeKind::MissingMetadata,
        None,
        "some tracks are missing: album artist",
    );
    let warning = notice.summary();
    source.notices.push(notice);
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let mut matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::None,
    );
    matched.warnings.push(warning.clone());

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert!(
        plan.warnings
            .iter()
            .all(|plan_warning| plan_warning.summary != warning)
    );
}

#[test]
fn existing_tags_keep_a_missing_metadata_warning_they_do_not_resolve() {
    let mut source = loose_source();
    let notice = crate::source::InspectionNotice::warning(
        crate::source::NoticeKind::MissingMetadata,
        None,
        "some tracks are missing: album artist",
    );
    let warning = notice.summary();
    source.notices.push(notice);
    let mut matched = result(MetadataSelection::ExistingTags, ArtworkSelection::None);
    matched.warnings.push(warning.clone());

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert!(
        plan.warnings
            .iter()
            .any(|plan_warning| plan_warning.summary == warning)
    );
}

#[test]
fn ancillary_collision_with_canonical_track_is_rejected_before_preview() {
    let mut source = loose_source();
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("01 - GROOVE IS IN THE HEART.MP3"),
        bytes: 1,
    });
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::None,
    );

    let error = build_plan(&source, &matched, Path::new("/library")).unwrap_err();

    assert!(matches!(error, PlanningError::Collision(_)));
}

#[test]
fn case_insensitive_file_directory_collision_is_rejected_before_preview() {
    let mut source = loose_source();
    source.ancillary.push(AncillaryFile {
        relative_path: PathBuf::from("01 - Groove Is in the Heart.mp3/notes.txt"),
        bytes: 1,
    });
    source.snapshot.push(crate::source::SourceSnapshotEntry {
        relative_path: PathBuf::from("01 - Groove Is in the Heart.mp3"),
        kind: crate::source::SourceObjectKind::Directory,
        bytes: 0,
        modified: None,
    });
    let candidate = single_candidate();
    let (inspection, _) = source_inspection(&source);
    let ranked = first_ranked(MatchPolicy::default().decide(&inspection, vec![candidate]));
    let matched = result(
        MetadataSelection::Provider(Box::new(ranked)),
        ArtworkSelection::None,
    );

    let error = build_plan(&source, &matched, Path::new("/library")).unwrap_err();

    assert!(matches!(error, PlanningError::Collision(_)));
}

fn loose_source() -> SourceInspection {
    SourceInspection {
        source: PathBuf::from("incoming/Groove.mp3"),
        kind: crate::domain::SourceKind::LooseFile,
        audio: vec![InspectedAudio {
            relative_path: PathBuf::from("Groove.mp3"),
            format: AudioFormat::Mp3,
            properties: AudioProperties {
                duration: Duration::from_secs(234),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: None,
                audio_bitrate: Some(192_000),
            },
            tags: AudioTags {
                title: Some("Groove Is in the Heart".into()),
                artist: Some("Deee-Lite".into()),
                ..AudioTags::default()
            },
        }],
        ancillary: Vec::new(),
        artwork: Vec::new(),
        selected_artwork: None,
        notices: Vec::new(),
        snapshot: Vec::new(),
    }
}

fn single_candidate() -> CandidateRelease {
    let credit = ArtistCredit::credited(
        "Deee-Lite",
        vec![Artist {
            name: "Deee-Lite".into(),
            musicbrainz_id: Some("artist-id".into()),
        }],
    );
    CandidateRelease {
        provider_key: "single".into(),
        title: "Groove Is in the Heart".into(),
        album_artist: credit.clone(),
        original_year: Some(1990),
        kind: ReleaseKind::Single,
        tracks: vec![
            ReleaseTrack {
                title: "Groove Is in the Heart".into(),
                artist_credit: credit.clone(),
                position: Position::new(1, 1),
                duration_ms: 234_000,
                recording_id: Some("recording-a".into()),
            },
            ReleaseTrack {
                title: "Power of Love".into(),
                artist_credit: credit,
                position: Position::new(1, 2),
                duration_ms: 220_000,
                recording_id: Some("recording-b".into()),
            },
        ],
        release_group_id: Some("group-id".into()),
        exact_release_id: None,
    }
}

fn first_ranked(decision: MatchDecision) -> crate::matching::RankedCandidate {
    match decision {
        MatchDecision::Selected { selected, .. } => *selected,
        MatchDecision::NeedsChoice(candidates) | MatchDecision::NoUsableMatch(candidates) => {
            candidates.into_iter().next().unwrap()
        }
    }
}

fn result(metadata: MetadataSelection, artwork: ArtworkSelection) -> GuidedMatchResult {
    GuidedMatchResult {
        metadata,
        metadata_provenance: MetadataProvenance::MusicBrainz,
        candidates: Vec::new(),
        artwork,
        archive_artwork: None,
        identification: None,
        warnings: Vec::new(),
        match_selection: MatchSelection::UserChosen,
    }
}
