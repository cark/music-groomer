use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::artwork_viewer::{ArtworkViewer, ViewerError};
use crate::domain::{ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack};
use crate::guided_matching::{ArtworkSelection, MetadataProvenance};
use crate::matching::{MatchDecision, MatchPolicy};
use crate::matching_ui::MetadataSelection;
use crate::plan::MatchSelection;
use crate::provider::source_inspection;
use crate::source::{SourceInspection, SourceInspector};

#[test]
fn one_guided_session_can_confirm_apply_and_exit_after_success() {
    let temporary = TempDir::new().unwrap();
    let (source_path, library, source, matched) = prepared_session(&temporary);
    let source_bytes = fs::read(&source_path).unwrap();
    let mut config = AppConfig::default();
    let mut interaction = Scripted::new(["a", ""]);
    let plan = choose_initial_destination(
        &mut interaction,
        &source,
        &matched,
        &mut config,
        Some(&library),
    )
    .unwrap()
    .unwrap();

    run_with_plan(
        &mut interaction,
        &source,
        matched,
        config,
        plan,
        &mut NoopViewer,
    )
    .unwrap();

    assert!(interaction.transcript.contains("Exact grooming preview"));
    assert!(interaction.transcript.contains("Applying confirmed plan"));
    assert!(interaction.transcript.contains("Grooming complete"));
    assert!(interaction.transcript.contains("Validation: passed"));
    assert_eq!(fs::read(source_path).unwrap(), source_bytes);
    assert!(
        library
            .join("Artist/2000 - Single/01 - Track.flac")
            .exists()
    );
}

#[test]
fn unchanged_destination_returns_to_preview_without_a_save_question() {
    let temporary = TempDir::new().unwrap();
    let (_, library, source, matched) = prepared_session(&temporary);
    let mut config = AppConfig::default();
    let mut interaction = Scripted::new([
        "d".to_owned(),
        String::new(),
        "d".to_owned(),
        library.display().to_string(),
        "c".to_owned(),
    ]);
    let plan = choose_initial_destination(
        &mut interaction,
        &source,
        &matched,
        &mut config,
        Some(&library),
    )
    .unwrap()
    .unwrap();

    run_with_plan(
        &mut interaction,
        &source,
        matched,
        config,
        plan,
        &mut NoopViewer,
    )
    .unwrap();

    let expected_prompt = format!("Destination root [{}]: ", library.display());
    assert_eq!(interaction.transcript.matches(&expected_prompt).count(), 2);
    assert!(!interaction.transcript.contains("Destination is valid."));
    assert!(!interaction.transcript.contains("Use and save as default"));
}

fn prepared_session(
    temporary: &TempDir,
) -> (PathBuf, PathBuf, SourceInspection, GuidedMatchResult) {
    let source_path = temporary.path().join("incoming.flac");
    let library = temporary.path().join("library");
    fs::copy(fixture("seed.flac"), &source_path).unwrap();
    fs::create_dir(&library).unwrap();
    let mut source = SourceInspector::default().inspect(&source_path).unwrap();
    source.audio[0].tags.title = Some("Track".into());
    source.audio[0].tags.artist = Some("Artist".into());
    let (inspection, _) = source_inspection(&source);
    let ranked = match MatchPolicy::default().decide(&inspection, vec![candidate()]) {
        MatchDecision::Selected { selected, .. } => *selected,
        MatchDecision::NeedsChoice(candidates) | MatchDecision::NoUsableMatch(candidates) => {
            candidates.into_iter().next().unwrap()
        }
    };
    let matched = GuidedMatchResult {
        metadata: MetadataSelection::Provider(Box::new(ranked)),
        metadata_provenance: MetadataProvenance::MusicBrainz,
        candidates: Vec::new(),
        artwork: ArtworkSelection::None,
        archive_artwork: None,
        identification: None,
        warnings: Vec::new(),
        match_selection: MatchSelection::UserChosen,
    };
    (source_path, library, source, matched)
}

fn candidate() -> CandidateRelease {
    CandidateRelease {
        provider_key: "candidate".into(),
        title: "Single".into(),
        album_artist: ArtistCredit::single("Artist"),
        original_year: Some(2000),
        kind: ReleaseKind::Single,
        tracks: vec![ReleaseTrack {
            title: "Track".into(),
            artist_credit: ArtistCredit::single("Artist"),
            position: Position::new(1, 1),
            duration_ms: 1000,
            recording_id: None,
        }],
        release_group_id: None,
        exact_release_id: None,
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}

struct Scripted {
    answers: VecDeque<String>,
    transcript: String,
}

impl Scripted {
    fn new<S: Into<String>>(answers: impl IntoIterator<Item = S>) -> Self {
        Self {
            answers: answers.into_iter().map(Into::into).collect(),
            transcript: String::new(),
        }
    }
}

impl Interaction for Scripted {
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

struct NoopViewer;

impl ArtworkViewer for NoopViewer {
    fn view_path(&mut self, _path: &Path) -> Result<(), ViewerError> {
        Ok(())
    }

    fn view_download(
        &mut self,
        _artwork: &crate::provider::ProviderArtwork,
    ) -> Result<(), ViewerError> {
        Ok(())
    }
}
