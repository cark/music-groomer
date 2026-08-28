use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{Artist, ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack};
use crate::guided_matching::{ArtworkSelection, GuidedMatchResult, MetadataProvenance};
use crate::matching::{MatchDecision, MatchPolicy};
use crate::matching_ui::MetadataSelection;
use crate::plan::{ArtworkOrigin, MatchSelection};
use crate::provider::{ProviderArtwork, source_inspection};
use crate::source::{
    ArtworkCandidate, ArtworkFormat, AudioFormat, AudioProperties, AudioTags, InspectedAudio,
    SourceInspection,
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
        relative_path: PathBuf::from("folder.png"),
        format: ArtworkFormat::Png,
        dimensions: (1000, 1000),
        name_priority: 2,
    });
    source.selected_artwork = Some(0);
    let matched = result(MetadataSelection::ExistingTags, ArtworkSelection::Source);

    let plan = build_plan(&source, &matched, Path::new("/library")).unwrap();

    assert_eq!(plan.artwork.output_name.as_deref(), Some("cover.png"));
    assert!(matches!(
        plan.artwork.origin,
        ArtworkOrigin::SourceSidecar { ref source_name } if source_name == "folder.png"
    ));
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
        identification: None,
        warnings: Vec::new(),
        match_selection: MatchSelection::UserChosen,
    }
}
