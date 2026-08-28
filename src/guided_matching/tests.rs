use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;
use std::{cell::Cell, rc::Rc};

use tempfile::TempDir;

use super::*;
use crate::artwork_viewer::{ArtworkViewer, ViewerError};
use crate::domain::{
    ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack, SourceKind,
};
use crate::fingerprint::{
    AudioFingerprint, AudioFingerprinter, FingerprintError, FingerprintProgress,
};
use crate::provider::{
    AcoustIdProvider, AcoustIdResponse, AcoustIdResult, ProviderError, ProviderSearch,
    ProviderSearchResult,
};
use crate::source::{
    AncillaryFile, ArtworkCandidate, ArtworkFormat, AudioFormat, AudioProperties, AudioTags,
    InspectedAudio,
};

struct ScriptedInteraction {
    answers: VecDeque<String>,
    transcript: String,
}

impl Interaction for ScriptedInteraction {
    fn present(&mut self, line: UiLine) -> io::Result<()> {
        self.transcript.push_str(&line.plain_text());
        self.transcript.push('\n');
        Ok(())
    }

    fn prompt(&mut self, prompt: UiLine) -> io::Result<String> {
        self.transcript.push_str(&prompt.plain_text());
        Ok(self.answers.pop_front().unwrap_or_default())
    }
}

struct FakeMetadata {
    results: VecDeque<Vec<CandidateRelease>>,
}

struct FakeFingerprinter {
    calls: usize,
}

struct QueuedFingerprinter {
    calls: usize,
    results: VecDeque<Result<AudioFingerprint, FingerprintError>>,
}

impl AudioFingerprinter for FakeFingerprinter {
    fn calculate(
        &mut self,
        _audio: &std::path::Path,
        progress: &mut dyn FingerprintProgress,
    ) -> Result<AudioFingerprint, FingerprintError> {
        self.calls += 1;
        progress.calculating(std::path::Path::new("track.flac"))?;
        Ok(AudioFingerprint {
            duration_seconds: 180,
            value: "fingerprint".into(),
        })
    }
}

impl AudioFingerprinter for QueuedFingerprinter {
    fn calculate(
        &mut self,
        _audio: &std::path::Path,
        progress: &mut dyn FingerprintProgress,
    ) -> Result<AudioFingerprint, FingerprintError> {
        self.calls += 1;
        progress.calculating(std::path::Path::new("track.flac"))?;
        self.results
            .pop_front()
            .unwrap_or(Err(FingerprintError::InvalidOutput(
                "fake result queue exhausted".into(),
            )))
    }
}

struct FakeAcoustId {
    score: f64,
}

struct QueuedAcoustId {
    responses: VecDeque<AcoustIdResponse>,
}

impl AcoustIdProvider for FakeAcoustId {
    fn lookup(
        &mut self,
        _fingerprint: &AudioFingerprint,
        progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError> {
        progress.event(ProviderEvent::Requesting {
            provider: crate::provider::ProviderName::AcoustId,
            operation: "fake AcoustID",
        })?;
        Ok(AcoustIdResponse {
            results: vec![AcoustIdResult {
                id: "acoustid-result".into(),
                score: self.score,
                recording_ids: vec!["recording".into()],
            }],
        })
    }
}

impl AcoustIdProvider for QueuedAcoustId {
    fn lookup(
        &mut self,
        _fingerprint: &AudioFingerprint,
        _progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError> {
        Ok(self.responses.pop_front().unwrap_or_default())
    }
}

struct ScriptedMetadata {
    results: VecDeque<Result<ProviderSearchResult, ProviderError>>,
}

impl MetadataProvider for ScriptedMetadata {
    fn search(
        &mut self,
        _search: &ProviderSearch,
        _progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError> {
        self.results.pop_front().unwrap_or_else(|| {
            Err(ProviderError::Network(
                "fake metadata result queue exhausted".into(),
            ))
        })
    }
}

impl MetadataProvider for FakeMetadata {
    fn search(
        &mut self,
        _search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError> {
        progress.event(ProviderEvent::Requesting {
            provider: crate::provider::ProviderName::MusicBrainz,
            operation: "fake metadata",
        })?;
        Ok(ProviderSearchResult {
            candidates: self.results.pop_front().unwrap_or_default(),
            warnings: Vec::new(),
        })
    }
}

struct NoArtwork;

struct QueuedArtwork {
    calls: Rc<Cell<usize>>,
    results: VecDeque<Option<crate::provider::ProviderArtwork>>,
}

struct NoopViewer;

impl ArtworkViewer for NoopViewer {
    fn view_path(&mut self, _path: &std::path::Path) -> Result<(), ViewerError> {
        Ok(())
    }

    fn view_download(
        &mut self,
        _artwork: &crate::provider::ProviderArtwork,
    ) -> Result<(), ViewerError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingViewer {
    downloads: usize,
    paths: Vec<PathBuf>,
}

impl ArtworkViewer for RecordingViewer {
    fn view_path(&mut self, path: &std::path::Path) -> Result<(), ViewerError> {
        self.paths.push(path.to_owned());
        Ok(())
    }

    fn view_download(
        &mut self,
        _artwork: &crate::provider::ProviderArtwork,
    ) -> Result<(), ViewerError> {
        self.downloads += 1;
        Ok(())
    }
}

impl ArtworkProvider for NoArtwork {
    fn front(
        &mut self,
        _release_group_id: &str,
        _progress: &mut dyn ProviderProgress,
    ) -> Result<Option<crate::provider::ProviderArtwork>, ProviderError> {
        Ok(None)
    }
}

impl ArtworkProvider for QueuedArtwork {
    fn front(
        &mut self,
        _release_group_id: &str,
        _progress: &mut dyn ProviderProgress,
    ) -> Result<Option<crate::provider::ProviderArtwork>, ProviderError> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.results.pop_front().unwrap_or(None))
    }
}

#[test]
fn clear_match_reaches_read_only_preview_without_extra_identifier_input() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album")]]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(matches!(result.metadata, MetadataSelection::Provider(_)));
    assert!(interaction.transcript.contains("Clear metadata match"));
    assert!(interaction.transcript.contains("Metadata preview"));
    assert!(
        interaction
            .transcript
            .contains("Choose Done to continue to the exact plan")
    );
}

#[test]
fn missing_destination_can_return_to_the_live_metadata_preview() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let destination = temporary.path().join("library");
    std::fs::create_dir(&destination).unwrap();
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from([
            "d".into(),
            "".into(),
            "d".into(),
            destination.display().to_string(),
            "o".into(),
        ]),
        transcript: String::new(),
    };
    let mut config = crate::config::AppConfig::default();
    let mut selected_plan = None;
    let source = source();

    run_with_identification_until(
        &mut interaction,
        &source,
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([vec![candidate("Album")]]),
            },
            NoArtwork,
            FakeFingerprinter { calls: 0 },
            FakeAcoustId { score: 0.95 },
        ),
        cache,
        &mut NoopViewer,
        |interaction, matched| {
            selected_plan = crate::guided_apply::choose_initial_destination(
                interaction,
                &source,
                matched,
                &mut config,
                None,
            )?;
            Ok(selected_plan.is_some())
        },
    )
    .unwrap();

    assert!(selected_plan.is_some());
    assert_eq!(
        interaction.transcript.matches("Metadata preview").count(),
        2
    );
    assert_eq!(
        interaction
            .transcript
            .matches("Checking metadata and provider cache")
            .count(),
        1
    );
}

#[test]
fn materially_changed_refresh_keeps_current_preview_by_default() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "1".into(), "".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album")], vec![candidate("Different Album")]]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    let MetadataSelection::Provider(selected) = result.metadata else {
        panic!("expected provider selection");
    };
    assert_eq!(selected.candidate.title, "Album");
    assert!(
        interaction
            .transcript
            .contains("Current preview kept unchanged")
    );
}

#[test]
fn artwork_view_action_uses_the_viewer_boundary() {
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::new(),
        transcript: String::new(),
    };
    let mut viewer = RecordingViewer::default();
    let artwork = crate::provider::ProviderArtwork {
        bytes: vec![1, 2, 3],
        format: crate::source::ArtworkFormat::Jpeg,
        dimensions: (10, 20),
    };

    artwork::view_artwork(
        &mut interaction,
        &source(),
        &ArtworkSelection::CoverArtArchive(artwork),
        &mut viewer,
    )
    .unwrap();

    assert_eq!(viewer.downloads, 1);
    assert!(interaction.transcript.contains("Opened the artwork choice"));
}

#[test]
fn artwork_chooser_lists_selects_and_views_every_available_choice() {
    let mut source = source();
    source.artwork = vec![
        ArtworkCandidate {
            relative_path: PathBuf::from("cover.jpg"),
            format: ArtworkFormat::Jpeg,
            dimensions: (400, 400),
            name_priority: 0,
        },
        ArtworkCandidate {
            relative_path: PathBuf::from("folder.png"),
            format: ArtworkFormat::Png,
            dimensions: (800, 600),
            name_priority: 1,
        },
    ];
    source.selected_artwork = Some(0);
    source.ancillary = vec![
        AncillaryFile {
            relative_path: PathBuf::from("cover.jpg"),
            bytes: 4096,
        },
        AncillaryFile {
            relative_path: PathBuf::from("folder.png"),
            bytes: 8192,
        },
    ];
    let archive = crate::provider::ProviderArtwork {
        bytes: vec![0; 2048],
        format: ArtworkFormat::Jpeg,
        dimensions: (1200, 1000),
    };
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from([
            "v".into(),
            "2".into(),
            "v".into(),
            "3".into(),
            "2".into(),
            "b".into(),
        ]),
        transcript: String::new(),
    };
    let mut viewer = RecordingViewer::default();

    let selected = artwork::choose_artwork(
        &mut interaction,
        &source,
        Some(&archive),
        ArtworkSelection::Source(0),
        &mut viewer,
    )
    .unwrap();

    assert_eq!(selected, ArtworkSelection::Source(1));
    assert_eq!(viewer.paths, [PathBuf::from("incoming/album/folder.png")]);
    assert_eq!(viewer.downloads, 1);
    assert!(
        interaction
            .transcript
            .contains("✓ 1. Source — cover.jpg — JPEG, 400×400, 4.0 KiB")
    );
    assert!(
        interaction
            .transcript
            .contains("2. Source — folder.png — PNG, 800×600, 8.0 KiB")
    );
    assert!(
        interaction
            .transcript
            .contains("3. Cover Art Archive — front — JPEG, 1200×1000, 2.0 KiB")
    );
    assert!(interaction.transcript.contains("✓ 2. Source — folder.png"));
    assert!(interaction.transcript.contains("View which artwork? [1-3]"));
}

#[test]
fn metadata_review_can_replace_an_automatic_selection_without_restarting() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut alternative = candidate("Alternative");
    alternative.original_year = Some(2001);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["r".into(), "m".into(), "2".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album"), alternative]]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    let MetadataSelection::Provider(selected) = result.metadata else {
        panic!("expected provider metadata");
    };
    assert_eq!(selected.candidate.title, "Alternative");
    assert_eq!(result.candidates.len(), 2);
    assert!(interaction.transcript.contains("Metadata selection"));
    assert!(interaction.transcript.contains("Tracks: Track"));
}

#[test]
fn refresh_checks_artwork_and_keeps_the_current_choice_when_declined() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let first = provider_artwork((10, 10));
    let changed = provider_artwork((20, 20));
    let calls = Rc::new(Cell::new(0));
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album")], vec![candidate("Album")]]),
        },
        QueuedArtwork {
            calls: calls.clone(),
            results: VecDeque::from([Some(first.clone()), Some(changed)]),
        },
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(calls.get(), 2);
    assert_eq!(result.artwork, ArtworkSelection::CoverArtArchive(first));
    assert!(interaction.transcript.contains("Previous: JPEG 10x10"));
    assert!(interaction.transcript.contains("Refreshed: JPEG 20x20"));
}

#[test]
fn existing_source_release_group_offers_artwork_but_does_not_select_it_automatically() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut source = source();
    source.audio[0].tags.release_group_id = Some("source-group".into());
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source,
        false,
        FakeMetadata {
            results: VecDeque::from([Vec::new()]),
        },
        QueuedArtwork {
            calls: Rc::new(Cell::new(0)),
            results: VecDeque::from([Some(provider_artwork((30, 30)))]),
        },
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(result.artwork, ArtworkSelection::None);
    assert_eq!(
        result.metadata_provenance,
        MetadataProvenance::ExistingTags {
            artwork_via_source_id: true
        }
    );
    assert!(interaction.transcript.contains("via existing source ID"));
    assert!(interaction.transcript.contains("Artwork alternative"));
    assert!(interaction.transcript.contains("JPEG 30x30"));
    assert!(!interaction.transcript.contains("1200px"));
}

#[test]
fn source_year_fallback_is_applied_only_after_selection_and_keeps_provenance() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut missing_year = candidate("Album");
    missing_year.original_year = None;
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![missing_year]]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(
        result.metadata_provenance,
        MetadataProvenance::MusicBrainzWithSourceYear(2000)
    );
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| warning.contains("source year 2000"))
            .count(),
        1
    );
    assert!(
        interaction
            .transcript
            .contains("Year provenance: source tags")
    );
}

#[test]
fn final_warning_set_includes_source_paths_and_deduplicates_causes() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut source = source();
    source
        .notices
        .push(crate::source::InspectionNotice::warning(
            crate::source::NoticeKind::StaleReference,
            Some(PathBuf::from("playlist.m3u")),
            "may refer to renamed audio",
        ));
    source
        .notices
        .push(crate::source::InspectionNotice::warning(
            crate::source::NoticeKind::StaleReference,
            Some(PathBuf::from("playlist.m3u")),
            "may refer to renamed audio",
        ));
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source,
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album")]]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| warning.contains("playlist.m3u"))
            .count(),
        1
    );
}

#[test]
fn inconsistent_source_identity_tags_are_visible_before_textual_fallback() {
    let (mut inspection, _) = source_inspection(&source());
    inspection.tracks[0].album_artist_ids = vec!["artist-one".into()];
    let mut second = inspection.tracks[0].clone();
    second.source_name = "02.flac".into();
    second.album_artist_ids = vec!["artist-two".into()];
    second.release_group_id = Some("other-group".into());
    inspection.tracks[0].release_group_id = Some("first-group".into());
    inspection.tracks.push(second);

    let warnings = warnings::identifier_warnings(&inspection);

    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("release-group"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("album-artist"))
    );
}

#[test]
fn strong_fingerprint_fallback_resolves_and_selects_a_single_in_one_interaction() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["d".into()]),
        transcript: String::new(),
    };
    let mut fingerprinter = FakeFingerprinter { calls: 0 };
    let mut acoustid = FakeAcoustId { score: 0.95 };

    let result = run_with_identification(
        &mut interaction,
        &poorly_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([Vec::new(), vec![single_candidate()]]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(fingerprinter.calls, 1);
    assert_eq!(
        result.metadata_provenance,
        MetadataProvenance::MusicBrainzWithFingerprint
    );
    assert!(result.identification.is_some());
    assert!(
        interaction
            .transcript
            .contains("Calculating local audio fingerprint")
    );
    assert!(interaction.transcript.contains("not the audio file"));
    assert!(
        interaction
            .transcript
            .contains("Audio fingerprint supports this recording")
    );
    assert!(interaction.transcript.contains("Clear metadata match"));
}

#[test]
fn merely_qualifying_fingerprint_requires_confirmation() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["y".into(), "d".into()]),
        transcript: String::new(),
    };
    let mut fingerprinter = FakeFingerprinter { calls: 0 };
    let mut acoustid = FakeAcoustId { score: 0.85 };

    run_with_identification(
        &mut interaction,
        &poorly_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([Vec::new(), vec![single_candidate()]]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(interaction.transcript.contains("Use this uncertain match?"));
}

#[test]
fn provider_refresh_recalculates_and_refreshes_fingerprint_identification() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "d".into()]),
        transcript: String::new(),
    };
    let mut fingerprinter = FakeFingerprinter { calls: 0 };
    let mut acoustid = FakeAcoustId { score: 0.95 };

    run_with_identification(
        &mut interaction,
        &poorly_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([
                    Vec::new(),
                    vec![single_candidate()],
                    vec![single_candidate()],
                ]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(fingerprinter.calls, 2);
    assert!(
        interaction
            .transcript
            .contains("AcoustID identification refreshed")
    );
    assert!(
        interaction
            .transcript
            .contains("MusicBrainz data refreshed")
    );
    assert!(interaction.transcript.contains("preview did not change"));
}

#[test]
fn refresh_retries_fingerprinting_after_the_initial_calculation_failed() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["".into(), "f".into(), "y".into(), "d".into()]),
        transcript: String::new(),
    };
    let mut fingerprinter = QueuedFingerprinter {
        calls: 0,
        results: VecDeque::from([Err(FingerprintError::Unavailable), Ok(test_fingerprint())]),
    };
    let mut acoustid = FakeAcoustId { score: 0.95 };

    let result = run_with_identification(
        &mut interaction,
        &minimally_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([Vec::new(), Vec::new(), vec![single_candidate()]]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert_eq!(fingerprinter.calls, 2);
    assert!(result.identification.is_some());
    assert_eq!(
        result.metadata_provenance,
        MetadataProvenance::MusicBrainzWithFingerprint
    );
}

#[test]
fn refresh_merges_textual_and_fingerprint_candidates() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "d".into()]),
        transcript: String::new(),
    };
    let mut textual = single_candidate();
    textual.provider_key = "textual".into();
    textual.title = "Textual alternative".into();
    textual.release_group_id = Some("textual-group".into());
    textual.tracks[0].recording_id = Some("different-recording".into());
    let mut second_textual = textual.clone();
    second_textual.provider_key = "second-textual".into();
    second_textual.title = "Second textual alternative".into();
    second_textual.release_group_id = Some("second-textual-group".into());
    second_textual.tracks[0].recording_id = Some("another-recording".into());
    let mut fingerprinter = FakeFingerprinter { calls: 0 };
    let mut acoustid = FakeAcoustId { score: 0.95 };

    let result = run_with_identification(
        &mut interaction,
        &minimally_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([
                    vec![textual.clone(), second_textual.clone()],
                    vec![single_candidate()],
                    vec![textual, second_textual],
                    vec![single_candidate()],
                ]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(
        result
            .candidates
            .iter()
            .any(|candidate| candidate.candidate.provider_key == "textual"),
        "candidate keys: {:?}\n{}",
        result
            .candidates
            .iter()
            .map(|candidate| &candidate.candidate.provider_key)
            .collect::<Vec<_>>(),
        interaction.transcript,
    );
    assert!(
        result
            .candidates
            .iter()
            .any(|candidate| candidate.candidate.provider_key == "single")
    );
}

#[test]
fn refreshed_ambiguity_warning_is_part_of_the_current_preview() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "y".into(), "d".into()]),
        transcript: String::new(),
    };
    let mut selected = single_candidate();
    selected.tracks[0].recording_id = Some("recording-1".into());
    let mut fingerprinter = FakeFingerprinter { calls: 0 };
    let mut acoustid = QueuedAcoustId {
        responses: VecDeque::from([
            acoustid_response(&["recording-1"]),
            acoustid_response(&[
                "recording-1",
                "recording-2",
                "recording-3",
                "recording-4",
                "recording-5",
                "recording-6",
            ]),
        ]),
    };

    let result = run_with_identification(
        &mut interaction,
        &poorly_tagged_track(),
        false,
        GuidedProviders::new(
            FakeMetadata {
                results: VecDeque::from([
                    Vec::new(),
                    vec![selected.clone()],
                    Vec::new(),
                    vec![selected],
                ]),
            },
            NoArtwork,
            &mut fingerprinter,
            &mut acoustid,
        ),
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("more than five qualifying"))
    );
}

#[test]
fn accepted_artwork_refresh_removes_the_no_artwork_warning() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "y".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        FakeMetadata {
            results: VecDeque::from([vec![candidate("Album")], vec![candidate("Album")]]),
        },
        QueuedArtwork {
            calls: Rc::new(Cell::new(0)),
            results: VecDeque::from([None, Some(provider_artwork((640, 640)))]),
        },
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(matches!(
        result.artwork,
        ArtworkSelection::CoverArtArchive(_)
    ));
    assert!(
        result
            .warnings
            .iter()
            .all(|warning| warning != "No album artwork is available")
    );
}

#[test]
fn accepted_successful_refresh_replaces_an_old_provider_failure_warning() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["".into(), "f".into(), "y".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        ScriptedMetadata {
            results: VecDeque::from([
                Err(ProviderError::Network("initial outage".into())),
                Ok(provider_result(vec![candidate("Album")], Vec::new())),
            ]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(
        result
            .warnings
            .iter()
            .all(|warning| !warning.contains("initial outage"))
    );
}

#[test]
fn failed_refresh_that_keeps_cached_metadata_keeps_its_fallback_warning() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        ScriptedMetadata {
            results: VecDeque::from([
                Ok(provider_result(vec![candidate("Album")], Vec::new())),
                Err(ProviderError::Network("refresh outage".into())),
            ]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("refresh outage") && warning.contains("cached data"))
    );
}

#[test]
fn declined_metadata_refresh_keeps_warnings_for_the_retained_preview() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut interaction = ScriptedInteraction {
        answers: VecDeque::from(["f".into(), "1".into(), "".into(), "d".into()]),
        transcript: String::new(),
    };

    let result = run(
        &mut interaction,
        &source(),
        false,
        ScriptedMetadata {
            results: VecDeque::from([
                Ok(provider_result(
                    vec![candidate("Album")],
                    vec!["initial metadata warning".into()],
                )),
                Ok(provider_result(
                    vec![candidate("Different Album")],
                    vec!["refreshed metadata warning".into()],
                )),
            ]),
        },
        NoArtwork,
        cache,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(result.warnings.contains(&"initial metadata warning".into()));
    assert!(
        !result
            .warnings
            .contains(&"refreshed metadata warning".into())
    );
}

fn provider_artwork(dimensions: (u32, u32)) -> crate::provider::ProviderArtwork {
    crate::provider::ProviderArtwork {
        bytes: vec![dimensions.0 as u8, dimensions.1 as u8],
        format: crate::source::ArtworkFormat::Jpeg,
        dimensions,
    }
}

fn provider_result(
    candidates: Vec<CandidateRelease>,
    warnings: Vec<String>,
) -> ProviderSearchResult {
    ProviderSearchResult {
        candidates,
        warnings,
    }
}

fn test_fingerprint() -> AudioFingerprint {
    AudioFingerprint {
        duration_seconds: 180,
        value: "fingerprint".into(),
    }
}

fn acoustid_response(recording_ids: &[&str]) -> AcoustIdResponse {
    AcoustIdResponse {
        results: vec![AcoustIdResult {
            id: "acoustid-result".into(),
            score: 0.95,
            recording_ids: recording_ids.iter().map(|id| (*id).into()).collect(),
        }],
    }
}

fn source() -> SourceInspection {
    SourceInspection {
        source: PathBuf::from("incoming/album"),
        kind: SourceKind::AlbumDirectory,
        audio: vec![InspectedAudio {
            relative_path: PathBuf::from("01.flac"),
            format: AudioFormat::Flac,
            properties: AudioProperties {
                duration: Duration::from_secs(120),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                audio_bitrate: None,
            },
            tags: AudioTags {
                title: Some("Track".into()),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                album_artist: Some("Artist".into()),
                date: Some("2000".into()),
                track: Some(1),
                disc: Some(1),
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

fn candidate(title: &str) -> CandidateRelease {
    CandidateRelease {
        provider_key: title.into(),
        title: title.into(),
        album_artist: ArtistCredit::single("Artist"),
        original_year: Some(2000),
        kind: ReleaseKind::Album,
        tracks: vec![ReleaseTrack {
            title: "Track".into(),
            artist_credit: ArtistCredit::single("Artist"),
            position: Position::new(1, 1),
            duration_ms: 120_000,
            recording_id: Some("recording".into()),
        }],
        release_group_id: Some(format!("group-{title}")),
        exact_release_id: None,
    }
}

fn poorly_tagged_track() -> SourceInspection {
    SourceInspection {
        source: PathBuf::from("incoming/unknown.flac"),
        kind: SourceKind::LooseFile,
        audio: vec![InspectedAudio {
            relative_path: PathBuf::from("unknown.flac"),
            format: AudioFormat::Flac,
            properties: AudioProperties {
                duration: Duration::from_secs(180),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                audio_bitrate: None,
            },
            tags: AudioTags::default(),
        }],
        ancillary: Vec::new(),
        artwork: Vec::new(),
        selected_artwork: None,
        notices: Vec::new(),
        snapshot: Vec::new(),
    }
}

fn minimally_tagged_track() -> SourceInspection {
    let mut source = poorly_tagged_track();
    source.audio[0].tags.title = Some("Identified Song".into());
    source.audio[0].tags.artist = Some("Identified Artist".into());
    source
}

fn single_candidate() -> CandidateRelease {
    CandidateRelease {
        provider_key: "single".into(),
        title: "Identified Song".into(),
        album_artist: ArtistCredit::single("Identified Artist"),
        original_year: Some(1999),
        kind: ReleaseKind::Single,
        tracks: vec![ReleaseTrack {
            title: "Identified Song".into(),
            artist_credit: ArtistCredit::single("Identified Artist"),
            position: Position::new(1, 1),
            duration_ms: 180_000,
            recording_id: Some("recording".into()),
        }],
        release_group_id: Some("single-group".into()),
        exact_release_id: None,
    }
}
