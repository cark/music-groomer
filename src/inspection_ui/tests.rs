use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use super::*;
use crate::source::SourceInspector;

struct ScriptedInteraction {
    answers: VecDeque<String>,
    transcript: String,
}

impl ScriptedInteraction {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|answer| (*answer).to_owned()).collect(),
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
        Ok(self.answers.pop_front().unwrap_or_default())
    }
}

#[test]
fn summary_and_review_show_exact_detected_changes() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let source = temporary.path().join("release");
    fs::create_dir(&source).expect("release directory should be created");
    fs::copy(fixture("seed.flac"), source.join("track.music"))
        .expect("audio fixture should be copied");
    RgbaImage::from_pixel(2, 3, Rgba([20, 40, 60, 255]))
        .save_with_format(source.join("folder.jpg"), image::ImageFormat::Png)
        .expect("artwork fixture should be created");
    fs::write(source.join("notes.txt"), "preserve me")
        .expect("ancillary fixture should be created");
    let inspection = SourceInspector::default()
        .inspect(&source)
        .expect("fixture should inspect");
    let mut interaction = ScriptedInteraction::new(&["r", "d"]);

    run(&mut interaction, &inspection).expect("guided review should complete");

    assert!(interaction.transcript.contains("source inspection"));
    assert!(interaction.transcript.contains("Review files and tags"));
    assert!(
        interaction
            .transcript
            .contains("Eventual filename correction: track.music → track.flac")
    );
    assert!(
        interaction
            .transcript
            .contains("Eventual sidecar: folder.jpg → cover.png")
    );
    assert!(
        interaction
            .transcript
            .contains("notes.txt (11 bytes, preserved)")
    );
    assert!(interaction.transcript.contains("no files were changed"));
    assert!(
        interaction
            .transcript
            .contains("No provider was contacted and no destination was accessed")
    );
}

#[test]
fn blockers_explain_the_path_cause_and_source_status() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::write(temporary.path().join("broken.flac"), "not audio")
        .expect("broken audio fixture should be created");
    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("fixture should produce an inspection");
    let mut interaction = ScriptedInteraction::new(&["d"]);

    run(&mut interaction, &inspection).expect("blocked inspection should still be reviewable");

    assert!(interaction.transcript.contains("Blocker [broken.flac]"));
    assert!(interaction.transcript.contains("cannot parse audio"));
    assert!(interaction.transcript.contains("source remains untouched"));
    assert!(inspection.is_blocked());
}

#[test]
fn unresolved_artwork_candidates_do_not_promise_multiple_sidecars() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac"))
        .expect("audio fixture should be copied");
    for name in ["cover.png", "COVER.gif"] {
        RgbaImage::from_pixel(2, 3, Rgba([20, 40, 60, 255]))
            .save(temporary.path().join(name))
            .expect("artwork fixture should be created");
    }
    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("fixture should inspect");
    let mut interaction = ScriptedInteraction::new(&["r", "d"]);

    run(&mut interaction, &inspection).expect("guided review should complete");

    assert!(
        interaction
            .transcript
            .contains("No canonical sidecar selected yet")
    );
    assert_eq!(
        interaction
            .transcript
            .matches("Preserved unchanged")
            .count(),
        2
    );
    assert!(!interaction.transcript.contains("Eventual sidecar"));
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}
