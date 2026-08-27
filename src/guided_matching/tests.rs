use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::artwork_viewer::{ArtworkViewer, ViewerError};
use crate::domain::{
    ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack, SourceKind,
};
use crate::provider::{ProviderError, ProviderSearch};
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
    ) -> Result<Vec<CandidateRelease>, ProviderError> {
        progress.event(ProviderEvent::Requesting("fake metadata"))?;
        Ok(self.results.pop_front().unwrap_or_default())
    }
}

struct NoArtwork;

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
