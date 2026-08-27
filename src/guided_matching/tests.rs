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
use crate::provider::{ProviderError, ProviderSearch, ProviderSearchResult};
use crate::source::{AudioFormat, AudioProperties, AudioTags, InspectedAudio};

struct ScriptedInteraction {
    answers: VecDeque<String>,
    transcript: String,
}

impl Interaction for ScriptedInteraction {
    fn show(&mut self, text: &str) -> io::Result<()> {
        self.transcript.push_str(text);
        self.transcript.push('\n');
        Ok(())
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        self.transcript.push_str(prompt);
        Ok(self.answers.pop_front().unwrap_or_default())
    }
}

struct FakeMetadata {
    results: VecDeque<Vec<CandidateRelease>>,
}

impl MetadataProvider for FakeMetadata {
    fn search(
        &mut self,
        _search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError> {
        progress.event(ProviderEvent::Requesting("fake metadata"))?;
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
}

impl ArtworkViewer for RecordingViewer {
    fn view_path(&mut self, _path: &std::path::Path) -> Result<(), ViewerError> {
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
    assert!(interaction.transcript.contains("metadata preview"));
    assert!(
        interaction
            .transcript
            .contains("Apply arrives in milestone 4")
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

    view_artwork(
        &mut interaction,
        &source(),
        &ArtworkSelection::CoverArtArchive(artwork),
        &mut viewer,
    )
    .unwrap();

    assert_eq!(viewer.downloads, 1);
    assert!(
        interaction
            .transcript
            .contains("Opened the selected artwork")
    );
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

    let warnings = identifier_warnings(&inspection);

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

fn provider_artwork(dimensions: (u32, u32)) -> crate::provider::ProviderArtwork {
    crate::provider::ProviderArtwork {
        bytes: vec![dimensions.0 as u8, dimensions.1 as u8],
        format: crate::source::ArtworkFormat::Jpeg,
        dimensions,
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
