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
use crate::source::SourceInspector;

#[test]
fn one_guided_session_can_confirm_apply_and_exit_after_success() {
    let temporary = TempDir::new().unwrap();
    let source_path = temporary.path().join("incoming.flac");
    let library = temporary.path().join("library");
    fs::copy(fixture("seed.flac"), &source_path).unwrap();
    fs::create_dir(&library).unwrap();
    let source_bytes = fs::read(&source_path).unwrap();
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
    let mut interaction = Scripted::new(["a", ""]);

    run(
        &mut interaction,
        &source,
        matched,
        AppConfig::default(),
        Some(&library),
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
    fn new(answers: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            answers: answers.into_iter().map(str::to_owned).collect(),
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
