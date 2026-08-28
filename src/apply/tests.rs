use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::plan::{
    AncillaryPlan, ArtworkChoice, ArtworkOrigin, GroomingPlan, MatchSelection, MetadataBasis,
    TrackPlan,
};
use crate::source::{PlannedTags, SourceInspector};

const FIXTURES: &[&str] = &[
    "seed.flac",
    "seed.mp3",
    "seed-aac.m4a",
    "seed-alac.m4a",
    "seed.ogg",
    "seed.opus",
];

#[test]
fn every_supported_format_survives_a_complete_apply_transaction() {
    for fixture in FIXTURES {
        let environment = Environment::new(fixture);
        let before = fs::read(&environment.source_path).unwrap();
        let mut progress = RecordedProgress::default();

        let report = environment
            .engine(false)
            .apply(&environment.inspection, &environment.plan, &mut progress)
            .unwrap_or_else(|error| panic!("{fixture} apply failed: {error}"));

        assert_eq!(report.destination, environment.plan.destination);
        assert_eq!(report.tracks_validated, 1);
        assert!(report.source_unchanged);
        assert_eq!(fs::read(&environment.source_path).unwrap(), before);
        assert!(!environment.plan.destination.join("01 - Groomed").exists());
        assert!(
            environment
                .plan
                .destination
                .join(format!(
                    "01 - Groomed.{}",
                    environment.inspection.audio[0].format.canonical_extension()
                ))
                .exists()
        );
        assert_eq!(
            progress.stages,
            [
                ApplyStage::Preflight,
                ApplyStage::Copying,
                ApplyStage::Grooming,
                ApplyStage::Validating,
                ApplyStage::Publishing,
            ]
        );
        assert!(
            fs::read_dir(&environment.temporary_root)
                .unwrap()
                .next()
                .is_none()
        );
    }
}

#[test]
fn forced_cross_filesystem_route_is_complete_and_cleans_both_work_areas() {
    let environment = Environment::new("seed.flac");

    let report = environment
        .engine(true)
        .apply(&environment.inspection, &environment.plan, &mut ())
        .unwrap();

    assert!(report.publication_copied);
    assert!(environment.plan.destination.exists());
    assert!(
        !environment
            .library
            .join(publication::PARTIAL_DIRECTORY)
            .exists()
    );
    assert!(
        fs::read_dir(&environment.temporary_root)
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn validated_album_replacement_retains_old_release_and_cleans_both_staging_areas() {
    let temporary = TempDir::new().unwrap();
    let library = temporary.path().join("library");
    let active = library.join("Test Artist/Old Album");
    let temporary_root = temporary.path().join("temporary");
    fs::create_dir_all(&active).unwrap();
    fs::create_dir(&temporary_root).unwrap();
    let source_track = active.join("seed.flac");
    fs::copy(fixture_path("seed.flac"), &source_track).unwrap();
    let old_bytes = fs::read(&source_track).unwrap();
    let inspection = SourceInspector::default().inspect(&active).unwrap();
    let plan = test_plan(&active, Path::new("seed.flac"), &library, "flac");
    let context = detect(&inspection, &plan).unwrap().unwrap();

    let report = ApplyEngine::in_temporary_root(temporary_root.clone())
        .apply_replacement(
            &inspection,
            &plan,
            &context,
            ReplacementRetention {
                display_label: "Test Artist — Old Album".into(),
                retained_at: 100,
                protected_until: 200,
            },
            &mut (),
        )
        .unwrap();

    assert!(!active.exists());
    assert!(plan.destination.join("01 - Groomed.flac").exists());
    assert_eq!(
        fs::read(report.replacement.retained_path.join("seed.flac")).unwrap(),
        old_bytes
    );
    assert!(!report.apply.source_unchanged);
    assert!(!library.join(publication::PARTIAL_DIRECTORY).exists());
    assert!(fs::read_dir(temporary_root).unwrap().next().is_none());
}

#[test]
fn changed_source_invalidates_preview_before_staging() {
    let environment = Environment::new("seed.flac");
    fs::write(&environment.source_path, b"changed after inspection").unwrap();

    let failure = environment
        .engine(false)
        .apply(&environment.inspection, &environment.plan, &mut ())
        .unwrap_err();

    assert_eq!(failure.stage, ApplyStage::Preflight);
    assert!(failure.requires_reinspection);
    assert!(!environment.plan.destination.exists());
    assert!(
        fs::read_dir(&environment.temporary_root)
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn existing_release_collision_is_never_overwritten() {
    let environment = Environment::new("seed.flac");
    fs::create_dir_all(&environment.plan.destination).unwrap();
    fs::write(environment.plan.destination.join("keep"), b"theirs").unwrap();

    let failure = environment
        .engine(false)
        .apply(&environment.inspection, &environment.plan, &mut ())
        .unwrap_err();

    assert!(failure.cause.contains("piece by piece"));
    assert_eq!(
        fs::read(environment.plan.destination.join("keep")).unwrap(),
        b"theirs"
    );
}

#[test]
fn album_result_cannot_be_published_inside_its_source() {
    let temporary = TempDir::new().unwrap();
    let album = temporary.path().join("album");
    let library_inside_source = album.join("library");
    fs::create_dir(&album).unwrap();
    fs::create_dir(&library_inside_source).unwrap();
    fs::copy(fixture_path("seed.flac"), album.join("source.flac")).unwrap();
    let inspection = SourceInspector::default().inspect(&album).unwrap();
    let plan = test_plan(
        &album,
        Path::new("source.flac"),
        &library_inside_source,
        "flac",
    );

    let failure = ApplyEngine::default()
        .apply(&inspection, &plan, &mut ())
        .unwrap_err();

    assert_eq!(failure.stage, ApplyStage::Preflight);
    assert!(failure.cause.contains("inside the selected source album"));
    assert!(!plan.destination.exists());
}

#[test]
fn failed_validation_cleans_staging_and_publishes_nothing() {
    let mut environment = Environment::new("seed.flac");
    environment.plan.artwork = ArtworkChoice {
        origin: ArtworkOrigin::CoverArtArchive {
            release_group_id: "group".into(),
        },
        label: "invalid test artwork".into(),
        dimensions: Some((10, 10)),
        output_name: Some("cover.jpg".into()),
    };
    environment.plan.archive_artwork_bytes = Some(b"not an image".to_vec());

    let failure = environment
        .engine(false)
        .apply(&environment.inspection, &environment.plan, &mut ())
        .unwrap_err();

    assert_eq!(failure.stage, ApplyStage::Validating);
    assert_eq!(failure.cleanup, CleanupOutcome::Complete);
    assert!(!environment.plan.destination.exists());
    assert!(
        fs::read_dir(&environment.temporary_root)
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn every_reported_stage_can_fail_without_publishing_or_leaking_staging() {
    for stage in [
        ApplyStage::Preflight,
        ApplyStage::Copying,
        ApplyStage::Grooming,
        ApplyStage::Validating,
        ApplyStage::Publishing,
    ] {
        let environment = Environment::new("seed.flac");
        let before = fs::read(&environment.source_path).unwrap();
        let mut progress = FailAt(stage);

        let failure = environment
            .engine(false)
            .apply(&environment.inspection, &environment.plan, &mut progress)
            .unwrap_err();

        assert_eq!(failure.stage, stage);
        assert_eq!(failure.cause, "injected stage failure");
        assert!(!environment.plan.destination.exists());
        assert_eq!(fs::read(&environment.source_path).unwrap(), before);
        assert!(
            fs::read_dir(&environment.temporary_root)
                .unwrap()
                .next()
                .is_none()
        );
        let expected_cleanup = if stage == ApplyStage::Preflight {
            CleanupOutcome::NotNeeded
        } else {
            CleanupOutcome::Complete
        };
        assert_eq!(failure.cleanup, expected_cleanup);
    }
}

#[test]
fn archive_artwork_replaces_source_cover_and_preserves_the_original() {
    let temporary = TempDir::new().unwrap();
    let album = temporary.path().join("album");
    let library = temporary.path().join("library");
    let temporary_root = temporary.path().join("temporary");
    fs::create_dir(&album).unwrap();
    fs::create_dir(&library).unwrap();
    fs::create_dir(&temporary_root).unwrap();
    let source_audio = album.join("source.flac");
    fs::copy(fixture_path("seed.flac"), &source_audio).unwrap();
    let source_artwork = jpeg_bytes(2, 2, [20, 40, 60]);
    fs::write(album.join("folder.jpg"), &source_artwork).unwrap();
    let source_audio_before = fs::read(&source_audio).unwrap();
    let inspection = SourceInspector::default().inspect(&album).unwrap();
    let archive_artwork = jpeg_bytes(4, 3, [180, 120, 60]);
    let mut plan = test_plan(&album, Path::new("source.flac"), &library, "flac");
    plan.ancillary = vec![AncillaryPlan {
        source_relative: PathBuf::from("folder.jpg"),
        destination_relative: PathBuf::from("original-artwork/folder.jpg"),
    }];
    plan.artwork = ArtworkChoice {
        origin: ArtworkOrigin::CoverArtArchive {
            release_group_id: "test-release-group".into(),
        },
        label: "Cover Art Archive front".into(),
        dimensions: Some((4, 3)),
        output_name: Some("cover.jpg".into()),
    };
    plan.archive_artwork_bytes = Some(archive_artwork.clone());

    let report = ApplyEngine::in_temporary_root(temporary_root)
        .apply(&inspection, &plan, &mut ())
        .unwrap();

    assert!(report.artwork_validated);
    assert_eq!(
        fs::read(plan.destination.join("cover.jpg")).unwrap(),
        archive_artwork
    );
    assert_eq!(
        fs::read(plan.destination.join("original-artwork/folder.jpg")).unwrap(),
        source_artwork
    );
    assert!(!plan.destination.join("folder.jpg").exists());
    assert_eq!(fs::read(&source_audio).unwrap(), source_audio_before);
    assert_eq!(fs::read(album.join("folder.jpg")).unwrap(), source_artwork);
}

#[test]
fn ancillary_files_and_empty_directories_are_preserved() {
    let temporary = TempDir::new().unwrap();
    let album = temporary.path().join("album");
    let library = temporary.path().join("library");
    let temporary_root = temporary.path().join("temporary");
    fs::create_dir_all(album.join("disc")).unwrap();
    fs::create_dir_all(album.join("extras/empty")).unwrap();
    fs::create_dir(&library).unwrap();
    fs::create_dir(&temporary_root).unwrap();
    fs::copy(fixture_path("seed.flac"), album.join("disc/source.flac")).unwrap();
    fs::write(album.join("extras/notes.txt"), b"keep me").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            album.join("extras/notes.txt"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(album.join("extras"), fs::Permissions::from_mode(0o555)).unwrap();
    }
    let inspection = SourceInspector::default().inspect(&album).unwrap();
    let destination = library.join("Test Artist/2000 - Test Album");
    let plan = GroomingPlan {
        source_label: album.display().to_string(),
        metadata: MetadataBasis::ExistingTags,
        match_selection: MatchSelection::UserChosen,
        match_reasons: vec!["test plan".into()],
        destination_root: library.clone(),
        destination: destination.clone(),
        tracks: vec![TrackPlan {
            source_relative: PathBuf::from("disc/source.flac"),
            destination: destination.join("01 - Groomed.flac"),
            tag_changes: Vec::new(),
            planned_tags: Some(tags()),
        }],
        ancillary: vec![AncillaryPlan {
            source_relative: PathBuf::from("extras/notes.txt"),
            destination_relative: PathBuf::from("extras/notes.txt"),
        }],
        ancillary_directories: vec![PathBuf::from("extras"), PathBuf::from("extras/empty")],
        artwork: ArtworkChoice {
            origin: ArtworkOrigin::None,
            label: "No sidecar artwork".into(),
            dimensions: None,
            output_name: None,
        },
        warnings: Vec::new(),
        preserved_embedded_artwork: 0,
        archive_artwork_bytes: None,
    };

    ApplyEngine::in_temporary_root(temporary_root)
        .apply(&inspection, &plan, &mut ())
        .unwrap();

    assert_eq!(
        fs::read(destination.join("extras/notes.txt")).unwrap(),
        b"keep me"
    );
    assert!(destination.join("extras/empty").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(album.join("extras"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_source_artwork_survives_preview_and_apply() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temporary = TempDir::new().unwrap();
    let album = temporary.path().join("album");
    let library = temporary.path().join("library");
    let temporary_root = temporary.path().join("temporary");
    fs::create_dir(&album).unwrap();
    fs::create_dir(&library).unwrap();
    fs::create_dir(&temporary_root).unwrap();
    fs::copy(fixture_path("seed.flac"), album.join("source.flac")).unwrap();
    let source_name = PathBuf::from(OsString::from_vec(b"f\x80lder.png".to_vec()));
    let source_artwork = album.join(&source_name);
    image::RgbImage::new(2, 2)
        .save_with_format(&source_artwork, image::ImageFormat::Png)
        .unwrap();
    let before = fs::read(&source_artwork).unwrap();
    let inspection = SourceInspector::default().inspect(&album).unwrap();
    let mut plan = test_plan(&album, Path::new("source.flac"), &library, "flac");
    plan.artwork = ArtworkChoice {
        origin: ArtworkOrigin::SourceSidecar {
            source_name: source_name.clone(),
        },
        label: format!("Source {}", source_name.display()),
        dimensions: Some((2, 2)),
        output_name: Some("cover.png".into()),
    };

    let preview = plan.artwork.description();
    assert!(preview.starts_with("Source "));
    ApplyEngine::in_temporary_root(temporary_root)
        .apply(&inspection, &plan, &mut ())
        .unwrap();

    assert_eq!(
        fs::read(plan.destination.join("cover.png")).unwrap(),
        before
    );
    assert_eq!(fs::read(source_artwork).unwrap(), before);
}

struct Environment {
    _temporary: TempDir,
    source_path: PathBuf,
    library: PathBuf,
    temporary_root: PathBuf,
    inspection: SourceInspection,
    plan: GroomingPlan,
}

impl Environment {
    fn new(fixture: &str) -> Self {
        let temporary = TempDir::new().unwrap();
        let incoming = temporary.path().join("incoming");
        let library = temporary.path().join("library");
        let temporary_root = temporary.path().join("temporary");
        fs::create_dir(&incoming).unwrap();
        fs::create_dir(&library).unwrap();
        fs::create_dir(&temporary_root).unwrap();
        let source_path = incoming.join(fixture);
        fs::copy(fixture_path(fixture), &source_path).unwrap();
        let inspection = SourceInspector::default().inspect(&source_path).unwrap();
        let extension = inspection.audio[0].format.canonical_extension();
        let plan = test_plan(&source_path, Path::new(fixture), &library, extension);
        Self {
            _temporary: temporary,
            source_path,
            library,
            temporary_root,
            inspection,
            plan,
        }
    }

    fn engine(&self, force_copy: bool) -> ApplyEngine {
        let engine = ApplyEngine::in_temporary_root(self.temporary_root.clone());
        if force_copy {
            engine.force_destination_copy()
        } else {
            engine
        }
    }
}

fn test_plan(
    source: &Path,
    source_relative: &Path,
    library: &Path,
    extension: &str,
) -> GroomingPlan {
    let destination = library.join("Test Artist/2000 - Test Album");
    GroomingPlan {
        source_label: source.display().to_string(),
        metadata: MetadataBasis::ExistingTags,
        match_selection: MatchSelection::UserChosen,
        match_reasons: vec!["test plan".into()],
        destination_root: library.to_owned(),
        destination: destination.clone(),
        tracks: vec![TrackPlan {
            source_relative: source_relative.to_owned(),
            destination: destination.join(format!("01 - Groomed.{extension}")),
            tag_changes: Vec::new(),
            planned_tags: Some(tags()),
        }],
        ancillary: Vec::new(),
        ancillary_directories: Vec::new(),
        artwork: ArtworkChoice {
            origin: ArtworkOrigin::None,
            label: "No sidecar artwork".into(),
            dimensions: None,
            output_name: None,
        },
        warnings: Vec::new(),
        preserved_embedded_artwork: 0,
        archive_artwork_bytes: None,
    }
}

fn tags() -> PlannedTags {
    PlannedTags {
        title: "Groomed".into(),
        artist: "Test Artist".into(),
        artists: vec!["Test Artist".into()],
        album: "Test Album".into(),
        album_artist: "Test Artist".into(),
        album_artists: vec!["Test Artist".into()],
        artist_ids: None,
        album_artist_ids: None,
        compilation: false,
        original_year: Some(2000),
        track: 1,
        track_total: 1,
        disc: 1,
        disc_total: 1,
        recording_id: None,
        release_group_id: None,
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}

#[derive(Default)]
struct RecordedProgress {
    stages: Vec<ApplyStage>,
}

impl ApplyProgress for RecordedProgress {
    fn stage(&mut self, stage: ApplyStage) -> Result<(), String> {
        self.stages.push(stage);
        Ok(())
    }
}

struct FailAt(ApplyStage);

impl ApplyProgress for FailAt {
    fn stage(&mut self, stage: ApplyStage) -> Result<(), String> {
        if stage == self.0 {
            Err("injected stage failure".into())
        } else {
            Ok(())
        }
    }
}

fn jpeg_bytes(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb(color));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Jpeg)
        .unwrap();
    bytes.into_inner()
}
