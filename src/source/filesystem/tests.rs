use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage, Rgba, RgbaImage};
use tempfile::TempDir;

use super::*;
use crate::source::{ArtworkFormat, LoftyAudioReader, PlannedTags};

#[test]
fn recursively_inventories_audio_ancillary_artwork_and_symlinks() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let source = temporary.path().join("release");
    let disc = source.join("Disc 1");
    fs::create_dir_all(&disc).expect("nested release should be created");
    fs::copy(fixture("seed.flac"), disc.join("01 song.audio"))
        .expect("audio fixture should be copied");
    fs::write(source.join(".rip-log"), "kept").expect("hidden ancillary should be created");
    fs::write(source.join("album.m3u"), "Disc 1/01 song.audio\n")
        .expect("playlist fixture should be created");
    write_image(&source.join("folder.jpg"), image::ImageFormat::Png);
    #[cfg(unix)]
    std::os::unix::fs::symlink("Disc 1/01 song.audio", source.join("shortcut"))
        .expect("symlink fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(&source)
        .expect("source should be inspectable");

    assert_eq!(inspection.audio.len(), 1);
    assert_eq!(
        inspection.audio[0].relative_path,
        Path::new("Disc 1/01 song.audio")
    );
    assert!(
        inspection
            .ancillary
            .iter()
            .any(|file| file.relative_path == Path::new(".rip-log"))
    );
    assert!(
        inspection
            .ancillary
            .iter()
            .any(|file| file.relative_path == Path::new("folder.jpg"))
    );
    assert_eq!(inspection.artwork.len(), 1);
    assert_eq!(inspection.artwork[0].format, ArtworkFormat::Png);
    assert_eq!(inspection.selected_artwork, Some(0));
    assert!(has_notice(&inspection, NoticeKind::ExtensionMismatch));
    assert!(has_notice(
        &inspection,
        NoticeKind::ArtworkExtensionMismatch
    ));
    assert!(has_notice(&inspection, NoticeKind::StaleReference));
    #[cfg(unix)]
    assert!(has_notice(&inspection, NoticeKind::SymlinkSkipped));
    assert!(!inspection.is_blocked());
}

#[test]
fn selecting_a_loose_file_excludes_siblings() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let selected = temporary.path().join("selected.flac");
    fs::copy(fixture("seed.flac"), &selected).expect("selected fixture should be copied");
    fs::copy(fixture("seed.mp3"), temporary.path().join("sibling.mp3"))
        .expect("sibling fixture should be copied");
    fs::write(temporary.path().join("cover.txt"), "not selected")
        .expect("sibling ancillary should be created");

    let inspection = SourceInspector::default()
        .inspect(&selected)
        .expect("loose file should be inspectable");

    assert_eq!(inspection.kind, SourceKind::LooseFile);
    assert_eq!(inspection.audio.len(), 1);
    assert!(inspection.ancillary.is_empty());
}

#[test]
fn inspection_progress_reports_each_ordinary_file_in_processing_order() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("b.flac")).unwrap();
    fs::write(temporary.path().join("a.log"), "kept").unwrap();
    let mut progress = RecordedProgress::default();

    SourceInspector::default()
        .inspect_with_progress(temporary.path(), &mut progress)
        .expect("directory should be inspectable");

    assert_eq!(
        progress
            .files
            .iter()
            .map(|(path, number, _)| (path.file_name().unwrap().to_owned(), *number))
            .collect::<Vec<_>>(),
        [("a.log".into(), 1), ("b.flac".into(), 2)]
    );
    assert!(progress.files.iter().all(|(_, _, bytes)| *bytes > 0));
}

#[test]
fn inspection_progress_failure_stops_with_its_cause() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac")).unwrap();
    let mut progress = FailingProgress;

    let error = SourceInspector::default()
        .inspect_with_progress(temporary.path(), &mut progress)
        .unwrap_err();

    assert!(matches!(error, InspectionError::Progress(message) if message == "display closed"));
}

#[test]
fn clearly_different_album_tags_block_accidental_batch_processing() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let first = temporary.path().join("one.flac");
    let second = temporary.path().join("two.flac");
    fs::copy(fixture("seed.flac"), &first).expect("first fixture should be copied");
    fs::copy(fixture("seed.flac"), &second).expect("second fixture should be copied");
    LoftyAudioReader
        .write_tags(&first, &plan("Album One", 1))
        .expect("first fixture should be tagged");
    LoftyAudioReader
        .write_tags(&second, &plan("Album Two", 2))
        .expect("second fixture should be tagged");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("directory should be inspectable");

    assert!(inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::MultipleReleases));
}

#[test]
fn cosmetic_album_title_differences_warn_without_blocking() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let first = temporary.path().join("one.flac");
    let second = temporary.path().join("two.flac");
    fs::copy(fixture("seed.flac"), &first).expect("first fixture should be copied");
    fs::copy(fixture("seed.flac"), &second).expect("second fixture should be copied");
    LoftyAudioReader
        .write_tags(&first, &plan("Evolution", 1))
        .expect("first fixture should be tagged");
    LoftyAudioReader
        .write_tags(&second, &plan("  EVOLUTION  ", 2))
        .expect("second fixture should be tagged");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("source should be inspectable");

    assert!(!inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::ContradictoryMetadata));
    assert!(!has_notice(&inspection, NoticeKind::MultipleReleases));
}

#[test]
fn cue_sheet_with_multiple_virtual_tracks_blocks() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("album.flac"))
        .expect("audio fixture should be copied");
    fs::write(
        temporary.path().join("album.cue"),
        "FILE \"album.flac\" WAVE\n  TRACK 01 AUDIO\n  TRACK 02 AUDIO\n",
    )
    .expect("cue fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("directory should be inspectable");

    assert!(inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::CueImage));
    assert!(
        inspection
            .ancillary
            .iter()
            .any(|file| file.relative_path == Path::new("album.cue"))
    );
}

#[test]
fn non_utf8_cue_sheet_still_blocks_a_multi_track_image() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("album.flac"))
        .expect("audio fixture should be copied");
    fs::write(
        temporary.path().join("album.cue"),
        b"FILE \"album.flac\" WAVE\nPERFORMER \"invalid \xff name\"\n  TRACK 01 AUDIO\n  track 02 AUDIO\n",
    )
    .expect("cue fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("source should be inspectable");

    assert!(inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::CueImage));
}

#[test]
fn equally_preferred_source_covers_remain_visible_and_unselected() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac"))
        .expect("audio fixture should be copied");
    write_image(&temporary.path().join("cover.png"), image::ImageFormat::Png);
    write_image(&temporary.path().join("COVER.gif"), image::ImageFormat::Gif);

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("directory should be inspectable");

    assert_eq!(inspection.artwork.len(), 2);
    assert_eq!(inspection.selected_artwork, None);
    assert!(has_notice(&inspection, NoticeKind::ArtworkChoiceRequired));
}

#[test]
fn supported_extension_with_invalid_contents_blocks() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::write(temporary.path().join("broken.flac"), "not audio")
        .expect("broken audio fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("directory should still produce an inspection");

    assert!(inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::CorruptAudio));
}

#[test]
fn ordinary_images_logs_and_single_track_cues_survive_failed_audio_probes() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac"))
        .expect("audio fixture should be copied");
    RgbImage::from_pixel(2, 3, Rgb([20, 40, 60]))
        .save_with_format(temporary.path().join("cover.jpg"), image::ImageFormat::Jpeg)
        .expect("JPEG fixture should be written");
    write_image(&temporary.path().join("scan.png"), image::ImageFormat::Png);
    fs::write(temporary.path().join("rip.log"), "accurate rip")
        .expect("log fixture should be created");
    fs::write(
        temporary.path().join("album.cue"),
        "FILE \"track.flac\" WAVE\n  TRACK 01 AUDIO\n",
    )
    .expect("cue fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("source should remain inspectable");

    assert!(!inspection.is_blocked());
    assert_eq!(inspection.audio.len(), 1);
    assert_eq!(inspection.artwork.len(), 1);
    for name in ["cover.jpg", "scan.png", "rip.log", "album.cue"] {
        assert!(
            inspection
                .ancillary
                .iter()
                .any(|file| file.relative_path == Path::new(name)),
            "{name} should be preserved as ancillary"
        );
    }
}

#[test]
fn readable_but_invalid_artwork_warns_and_is_preserved() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac"))
        .expect("audio fixture should be copied");
    let path = temporary.path().join("cover.png");
    write_image(&path, image::ImageFormat::Png);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("artwork fixture should open");
    file.set_len(40)
        .expect("artwork fixture should be truncated");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("source should remain inspectable");

    assert!(!inspection.is_blocked());
    assert!(has_notice(&inspection, NoticeKind::UnsupportedImage));
    assert!(
        inspection
            .ancillary
            .iter()
            .any(|file| file.relative_path == Path::new("cover.png"))
    );
    assert!(inspection.artwork.is_empty());
}

#[cfg(unix)]
#[test]
fn special_files_block_with_their_path() {
    use std::os::unix::net::UnixListener;

    let temporary = TempDir::new().expect("temporary directory should be created");
    fs::copy(fixture("seed.flac"), temporary.path().join("track.flac"))
        .expect("audio fixture should be copied");
    let socket_path = temporary.path().join("player.socket");
    let _listener = UnixListener::bind(&socket_path).expect("socket fixture should be created");

    let inspection = SourceInspector::default()
        .inspect(temporary.path())
        .expect("directory should still produce an inspection");

    let notice = inspection
        .notices
        .iter()
        .find(|notice| notice.kind == NoticeKind::SpecialFile)
        .expect("special object should be reported");
    assert_eq!(notice.path.as_deref(), Some(Path::new("player.socket")));
    assert_eq!(notice.severity, super::super::NoticeSeverity::Blocker);
}

fn has_notice(inspection: &SourceInspection, kind: NoticeKind) -> bool {
    inspection.notices.iter().any(|notice| notice.kind == kind)
}

#[derive(Default)]
struct RecordedProgress {
    files: Vec<(PathBuf, usize, u64)>,
}

impl InspectionProgress for RecordedProgress {
    fn inspecting_file(&mut self, path: &Path, number: usize, bytes: u64) -> Result<(), String> {
        self.files.push((path.to_owned(), number, bytes));
        Ok(())
    }
}

struct FailingProgress;

impl InspectionProgress for FailingProgress {
    fn inspecting_file(&mut self, _path: &Path, _number: usize, _bytes: u64) -> Result<(), String> {
        Err("display closed".into())
    }
}

fn write_image(path: &Path, format: image::ImageFormat) {
    let image = RgbaImage::from_pixel(2, 3, Rgba([20, 40, 60, 255]));
    image
        .save_with_format(path, format)
        .expect("image fixture should be written");
}

fn plan(album: &str, track: u32) -> PlannedTags {
    PlannedTags {
        title: format!("Track {track}"),
        artist: "Artist".to_owned(),
        artists: vec!["Artist".to_owned()],
        album: album.to_owned(),
        album_artist: "Artist".to_owned(),
        album_artists: vec!["Artist".to_owned()],
        artist_ids: None,
        album_artist_ids: None,
        compilation: false,
        original_year: Some(2000),
        track,
        track_total: 2,
        disc: 1,
        disc_total: 1,
        recording_id: None,
        release_group_id: None,
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}
