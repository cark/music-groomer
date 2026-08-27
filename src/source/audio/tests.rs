use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};
use tempfile::TempDir;

use super::*;

const FIXTURES: &[(&str, AudioFormat)] = &[
    ("seed.flac", AudioFormat::Flac),
    ("seed.mp3", AudioFormat::Mp3),
    ("seed-aac.m4a", AudioFormat::M4aAac),
    ("seed-alac.m4a", AudioFormat::M4aAlac),
    ("seed.ogg", AudioFormat::OggVorbis),
    ("seed.opus", AudioFormat::Opus),
];

const PICTURE_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207, 192, 240, 31, 0, 5,
    0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[test]
fn recognizes_every_claimed_fixture_from_its_contents() {
    let reader = LoftyAudioReader;
    for (name, expected) in FIXTURES {
        let path = fixture(name);
        let AudioProbe::Supported(audio) = reader.probe(&path).expect("fixture should parse")
        else {
            panic!("{} was not recognized as supported audio", path.display());
        };
        assert_eq!(audio.format, *expected, "wrong format for {name}");
        assert!(audio.properties.duration.as_millis() > 0);
        assert!(audio.properties.sample_rate.is_some());
        assert!(audio.properties.channels.is_some());
    }
}

#[test]
fn every_claimed_format_satisfies_the_preservation_contract() {
    for (name, expected_format) in FIXTURES {
        prove_preservation(name, *expected_format);
    }
}

fn prove_preservation(name: &str, expected_format: AudioFormat) {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let path = temporary.path().join(name);
    fs::copy(fixture(name), &path).expect("fixture should be copied before mutation");
    add_preserved_data(&path);

    let reader = LoftyAudioReader;
    let before = inspected(&reader, &path);
    let unrelated_before = unrelated(&path);
    let plan = plan();
    reader
        .write_tags(&path, &plan)
        .unwrap_or_else(|error| panic!("{name} should be writable: {error}"));
    let after = inspected(&reader, &path);
    let unrelated_after = unrelated(&path);

    assert_eq!(after.format, expected_format);
    assert_eq!(
        after.properties, before.properties,
        "properties changed for {name}"
    );
    assert_eq!(
        unrelated_after, unrelated_before,
        "unrelated data changed for {name}"
    );
    assert_eq!(after.tags.title.as_deref(), Some("New title"));
    assert_eq!(after.tags.artist.as_deref(), Some("Person A with Person B"));
    assert_eq!(after.tags.artists, ["Person A", "Person B"]);
    assert_eq!(after.tags.album.as_deref(), Some("New album"));
    assert_eq!(
        after.tags.album_artist.as_deref(),
        Some("Person A & Person B")
    );
    assert_eq!(after.tags.album_artists, ["Person A", "Person B"]);
    assert_eq!(after.tags.date.as_deref(), Some("1971"));
    assert_eq!(after.tags.track, Some(2));
    assert_eq!(after.tags.track_total, Some(8));
    assert_eq!(after.tags.disc, Some(1));
    assert_eq!(after.tags.disc_total, Some(1));
    assert_eq!(
        after.tags.recording_id.as_deref(),
        Some("recording-id"),
        "recording ID was not written for {name}"
    );
    assert_eq!(
        after.tags.release_group_id.as_deref(),
        Some("release-group-id"),
        "release-group ID was not written for {name}"
    );

    if expected_format == AudioFormat::Mp3 {
        let bytes = fs::read(&path).expect("written MP3 should be readable");
        assert_eq!(&bytes[..4], b"ID3\x04", "groomed MP3 should use ID3v2.4");
    }
}

fn add_preserved_data(path: &Path) {
    let mut tagged = Probe::open(path)
        .expect("fixture should open")
        .guess_file_type()
        .expect("fixture type should be detected")
        .read()
        .expect("fixture should parse");
    let tag_type = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().expect("primary tag should exist");
    tag.set_genre("Jazz".to_owned());
    assert!(tag.insert_text(ItemKey::ReplayGainTrackGain, "-3.25 dB".to_owned()));
    tag.insert_text(ItemKey::Comment, "keep this comment".to_owned());
    tag.push_picture(
        Picture::unchecked(PICTURE_BYTES.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .description("source front")
            .build(),
    );
    tagged
        .save_to_path(path, WriteOptions::new())
        .expect("preservation baseline should be writable");
}

fn unrelated(path: &Path) -> UnrelatedSnapshot {
    let tagged = Probe::open(path)
        .expect("fixture should open")
        .guess_file_type()
        .expect("fixture type should be detected")
        .read()
        .expect("fixture should parse");
    let tag = tagged.primary_tag().expect("primary tag should exist");
    UnrelatedSnapshot {
        genre: tag.genre().map(|value| value.into_owned()),
        replay_gain: tag
            .get_strings(ItemKey::ReplayGainTrackGain)
            .map(str::to_owned)
            .collect(),
        comments: tag
            .get_strings(ItemKey::Comment)
            .map(str::to_owned)
            .collect(),
        pictures: tag
            .pictures()
            .iter()
            .map(|picture| PictureSnapshot {
                data: picture.data().to_vec(),
                mime: picture.mime_type().map(|mime| format!("{mime:?}")),
                kind: picture.pic_type(),
                description: picture.description().map(str::to_owned),
            })
            .collect(),
    }
}

fn inspected(reader: &LoftyAudioReader, path: &Path) -> InspectedAudio {
    let AudioProbe::Supported(audio) = reader.probe(path).expect("fixture should parse") else {
        panic!("fixture should remain supported");
    };
    *audio
}

fn plan() -> PlannedTags {
    PlannedTags {
        title: "New title".to_owned(),
        artist: "Person A with Person B".to_owned(),
        artists: vec!["Person A".to_owned(), "Person B".to_owned()],
        album: "New album".to_owned(),
        album_artist: "Person A & Person B".to_owned(),
        album_artists: vec!["Person A".to_owned(), "Person B".to_owned()],
        original_year: 1971,
        track: 2,
        track_total: 8,
        disc: 1,
        disc_total: 1,
        recording_id: Some("recording-id".to_owned()),
        release_group_id: Some("release-group-id".to_owned()),
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}

#[derive(Debug, PartialEq, Eq)]
struct UnrelatedSnapshot {
    genre: Option<String>,
    replay_gain: Vec<String>,
    comments: Vec<String>,
    pictures: Vec<PictureSnapshot>,
}

#[derive(Debug, PartialEq, Eq)]
struct PictureSnapshot {
    data: Vec<u8>,
    mime: Option<String>,
    kind: PictureType,
    description: Option<String>,
}
