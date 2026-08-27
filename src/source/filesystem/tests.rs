use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
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
        original_year: 2000,
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
