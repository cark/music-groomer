use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

struct ScriptedInteraction {
    answers: VecDeque<String>,
    transcript: String,
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("music-groomer-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).expect("temporary test directory should be creatable");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).expect("temporary test directory should be removable");
    }
}

impl ScriptedInteraction {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|answer| (*answer).into()).collect(),
            transcript: String::new(),
        }
    }
}

impl Interaction for ScriptedInteraction {
    fn show(&mut self, text: &str) -> io::Result<()> {
        self.transcript.push_str(text);
        self.transcript.push('\n');
        Ok(())
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        self.transcript.push_str(prompt);
        self.answers
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "script ended"))
    }
}

#[test]
fn confident_match_reaches_apply_without_match_question() {
    let mut interaction = ScriptedInteraction::new(&["a", ""]);
    let output = std::env::temp_dir();

    let outcome = run(
        &mut interaction,
        Some(DemoScenario::ConfidentAlbum),
        Some(&output),
    )
    .expect("demo should finish");

    assert!(matches!(outcome, DemoOutcome::Applied(_)));
    assert!(interaction.transcript.contains("Matched automatically"));
    assert!(!interaction.transcript.contains("Which looks right?"));
    assert!(
        interaction.transcript.contains(
            &output
                .join("The Group/1971 - The Album")
                .display()
                .to_string()
        )
    );
    assert!(interaction.transcript.contains("No files were written"));
    assert!(interaction.transcript.contains("[Y/n]"));
}

#[test]
fn ambiguity_is_resolved_in_the_same_session_with_human_labels() {
    let mut interaction = ScriptedInteraction::new(&["2", "a", "yes"]);

    let outcome = run(
        &mut interaction,
        Some(DemoScenario::AmbiguousCollaboration),
        None,
    )
    .expect("demo should finish");

    assert!(matches!(outcome, DemoOutcome::Applied(_)));
    assert!(interaction.transcript.contains("Which looks right?"));
    assert!(
        interaction
            .transcript
            .contains("Niels-Henning Ørsted Pedersen & Kenny Drew — Duo: Studio Session (1974")
    );
    assert!(!interaction.transcript.contains("duo-session"));
}

#[test]
fn review_and_artwork_change_return_to_the_same_preview() {
    let mut interaction = ScriptedInteraction::new(&["r", "w", "v", "2", "2", "a", ""]);

    let outcome = run(&mut interaction, Some(DemoScenario::ConfidentAlbum), None)
        .expect("demo should finish");

    assert!(matches!(outcome, DemoOutcome::Applied(_)));
    assert!(interaction.transcript.contains("All planned changes"));
    assert!(interaction.transcript.contains("MusicBrainz artist IDs"));
    assert!(
        interaction
            .transcript
            .contains("Would open Cover Art Archive")
    );
    assert!(
        interaction
            .transcript
            .contains("Selected: Cover Art Archive")
    );
}

#[test]
fn unmatched_loose_track_is_visibly_unverified_and_can_be_cancelled() {
    let mut interaction = ScriptedInteraction::new(&["c"]);

    let outcome = run(&mut interaction, Some(DemoScenario::StandaloneTrack), None)
        .expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(
        interaction
            .transcript
            .contains("No matching single was found")
    );
    assert!(
        interaction
            .transcript
            .contains("not verified against MusicBrainz")
    );
    assert!(
        interaction
            .transcript
            .contains("Standalone Tracks/Mystery Song")
    );
}

#[test]
fn declining_final_confirmation_returns_to_preview() {
    let mut interaction = ScriptedInteraction::new(&["a", "n", "c"]);

    let outcome =
        run(&mut interaction, Some(DemoScenario::MatchedSingle), None).expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(
        interaction
            .transcript
            .contains("Apply not confirmed; returning to the preview")
    );
    assert!(
        interaction
            .transcript
            .contains("Artwork: Cover Art Archive front image (1200x1200)")
    );
}

#[test]
fn empty_main_action_redisplays_the_preview_instead_of_cancelling() {
    let mut interaction = ScriptedInteraction::new(&["", "c"]);

    let outcome = run(&mut interaction, Some(DemoScenario::ConfidentAlbum), None)
        .expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert_eq!(interaction.transcript.matches("Preview\n").count(), 2);
}

#[test]
fn destination_can_be_validated_and_changed_inside_the_session() {
    let temporary_root = std::env::temp_dir();
    let root = temporary_root.display().to_string();
    let mut interaction = ScriptedInteraction::new(&["d", &root, "o", "c"]);

    let outcome = run(&mut interaction, Some(DemoScenario::ConfidentAlbum), None)
        .expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(interaction.transcript.contains("Destination is valid"));
    assert!(
        interaction.transcript.contains(
            &temporary_root
                .join("The Group/1971 - The Album")
                .display()
                .to_string()
        )
    );
}

#[test]
fn saving_a_new_default_is_explicitly_simulated() {
    let root = std::env::temp_dir().display().to_string();
    let mut interaction = ScriptedInteraction::new(&["d", &root, "s", "c"]);

    let outcome =
        run(&mut interaction, Some(DemoScenario::MatchedSingle), None).expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(interaction.transcript.contains("Demo only: would save"));
}

#[test]
fn destination_change_refuses_an_existing_final_album() {
    let temporary_root = TemporaryDirectory::new();
    fs::create_dir_all(temporary_root.path().join("The Group/1971 - The Album"))
        .expect("collision fixture should be creatable");
    let root = temporary_root.path().display().to_string();
    let mut interaction = ScriptedInteraction::new(&["d", &root, "b", "c"]);

    let outcome = run(&mut interaction, Some(DemoScenario::ConfidentAlbum), None)
        .expect("demo should return to preview after collision");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(
        interaction
            .transcript
            .contains("final album path already exists")
    );
}

#[test]
fn scenario_selection_requires_an_explicit_cancel() {
    let mut interaction = ScriptedInteraction::new(&["", "c"]);

    let outcome = run(&mut interaction, None, None).expect("demo should finish");

    assert_eq!(outcome, DemoOutcome::Cancelled);
    assert!(
        interaction
            .transcript
            .contains("Please enter 1, 2, 3, 4, or c")
    );
}
