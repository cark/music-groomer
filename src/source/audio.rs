use std::fmt;
use std::fs::File;
use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::id3::v2::Id3v2Tag;
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::{Layer, MpegFile};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagItem, TagType};

use super::{AudioFormat, AudioProperties, AudioTags, InspectedAudio};

pub(super) enum AudioProbe {
    Supported(Box<InspectedAudio>),
    Unsupported(String),
    NotAudio,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlannedTags {
    pub title: String,
    pub artist: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artist: String,
    pub album_artists: Vec<String>,
    pub original_year: u16,
    pub track: u32,
    pub track_total: u32,
    pub disc: u32,
    pub disc_total: u32,
    pub recording_id: Option<String>,
    pub release_group_id: Option<String>,
}

#[derive(Debug)]
pub enum AudioReadError {
    Io(std::io::Error),
    Parse(String),
    Write(String),
}

impl fmt::Display for AudioReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Parse(error) => write!(formatter, "cannot parse audio: {error}"),
            Self::Write(error) => write!(formatter, "cannot write tags: {error}"),
        }
    }
}

impl std::error::Error for AudioReadError {}

impl From<std::io::Error> for AudioReadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoftyAudioReader;

impl LoftyAudioReader {
    pub(super) fn probe(&self, path: &Path) -> Result<AudioProbe, AudioReadError> {
        let probe = Probe::open(path)
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .guess_file_type()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?;
        let Some(file_type) = probe.file_type() else {
            return Ok(AudioProbe::NotAudio);
        };
        let format = match supported_format(path, file_type)? {
            Some(format) => format,
            None => return Ok(AudioProbe::Unsupported(format!("{file_type:?}"))),
        };
        let tagged = probe
            .read()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?;
        let properties = tagged.properties();
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
        let tags = tag.map_or_else(AudioTags::default, read_tags);

        Ok(AudioProbe::Supported(Box::new(InspectedAudio {
            relative_path: path.to_owned(),
            format,
            properties: AudioProperties {
                duration: properties.duration(),
                sample_rate: properties.sample_rate(),
                channels: properties.channels(),
                bit_depth: properties.bit_depth(),
                audio_bitrate: properties.audio_bitrate(),
            },
            tags,
        })))
    }

    pub fn write_tags(&self, path: &Path, plan: &PlannedTags) -> Result<(), AudioReadError> {
        let mut tagged = Probe::open(path)
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .guess_file_type()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .read()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?;
        let tag_type = tagged.primary_tag_type();
        if tagged.primary_tag().is_none() {
            tagged.insert_tag(Tag::new(tag_type));
        }
        let tag = tagged
            .primary_tag_mut()
            .expect("the primary tag was inserted above");
        apply_plan(tag, plan);
        tagged
            .save_to_path(path, WriteOptions::new())
            .map_err(|error| AudioReadError::Write(error.to_string()))?;
        if tag_type == TagType::Id3v2 {
            write_id3v2_release_group(path, plan.release_group_id.as_deref())?;
        }
        Ok(())
    }
}

fn supported_format(
    path: &Path,
    file_type: FileType,
) -> Result<Option<AudioFormat>, AudioReadError> {
    match file_type {
        FileType::Flac => Ok(Some(AudioFormat::Flac)),
        FileType::Opus => Ok(Some(AudioFormat::Opus)),
        FileType::Vorbis => Ok(Some(AudioFormat::OggVorbis)),
        FileType::Mpeg => mpeg_format(path),
        FileType::Mp4 => mp4_format(path),
        _ => Ok(None),
    }
}

fn mpeg_format(path: &Path) -> Result<Option<AudioFormat>, AudioReadError> {
    let mut file = File::open(path)?;
    let mpeg = MpegFile::read_from(&mut file, ParseOptions::new())
        .map_err(|error| AudioReadError::Parse(error.to_string()))?;
    Ok((*mpeg.properties().layer() == Layer::Layer3).then_some(AudioFormat::Mp3))
}

fn mp4_format(path: &Path) -> Result<Option<AudioFormat>, AudioReadError> {
    let mut file = File::open(path)?;
    let mp4 = Mp4File::read_from(&mut file, ParseOptions::new())
        .map_err(|error| AudioReadError::Parse(error.to_string()))?;
    Ok(match mp4.properties().codec() {
        Some(Mp4Codec::AAC) => Some(AudioFormat::M4aAac),
        Some(Mp4Codec::ALAC) => Some(AudioFormat::M4aAlac),
        _ => None,
    })
}

fn read_tags(tag: &Tag) -> AudioTags {
    AudioTags {
        title: text(tag.title()),
        artist: text(tag.artist()),
        artists: strings(tag, ItemKey::TrackArtists),
        album: text(tag.album()),
        album_artist: tag.get_string(ItemKey::AlbumArtist).map(str::to_owned),
        album_artists: strings(tag, ItemKey::AlbumArtists),
        date: tag
            .get_string(ItemKey::OriginalReleaseDate)
            .or_else(|| tag.get_string(ItemKey::RecordingDate))
            .or_else(|| tag.get_string(ItemKey::Year))
            .map(str::to_owned),
        track: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
        disc_total: tag.disk_total(),
        recording_id: tag
            .get_string(ItemKey::MusicBrainzRecordingId)
            .map(str::to_owned),
        release_group_id: tag
            .get_string(ItemKey::MusicBrainzReleaseGroupId)
            .map(str::to_owned),
        embedded_pictures: tag.pictures().len(),
    }
}

fn text(value: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    value.map(|value| value.into_owned())
}

fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key).map(str::to_owned).collect()
}

fn apply_plan(tag: &mut Tag, plan: &PlannedTags) {
    tag.set_title(plan.title.clone());
    tag.set_artist(plan.artist.clone());
    replace_text_values(tag, ItemKey::TrackArtists, &plan.artists);
    tag.set_album(plan.album.clone());
    tag.insert_text(ItemKey::AlbumArtist, plan.album_artist.clone());
    replace_text_values(tag, ItemKey::AlbumArtists, &plan.album_artists);
    tag.insert_text(ItemKey::OriginalReleaseDate, plan.original_year.to_string());
    tag.insert_text(ItemKey::RecordingDate, plan.original_year.to_string());
    tag.set_track(plan.track);
    tag.set_track_total(plan.track_total);
    tag.set_disk(plan.disc);
    tag.set_disk_total(plan.disc_total);
    replace_optional(tag, ItemKey::MusicBrainzRecordingId, &plan.recording_id);
    replace_optional(
        tag,
        ItemKey::MusicBrainzReleaseGroupId,
        &plan.release_group_id,
    );
}

fn replace_text_values(tag: &mut Tag, key: ItemKey, values: &[String]) {
    tag.take(key).for_each(drop);
    for value in values {
        tag.push(TagItem::new(
            key,
            lofty::tag::ItemValue::Text(value.clone()),
        ));
    }
}

fn replace_optional(tag: &mut Tag, key: ItemKey, value: &Option<String>) {
    tag.take(key).for_each(drop);
    if let Some(value) = value {
        if key == ItemKey::MusicBrainzRecordingId {
            tag.insert_unchecked(TagItem::new(
                key,
                lofty::tag::ItemValue::Text(value.clone()),
            ));
        } else {
            tag.insert_text(key, value.clone());
        }
    }
}

fn write_id3v2_release_group(
    path: &Path,
    release_group_id: Option<&str>,
) -> Result<(), AudioReadError> {
    let tagged = Probe::open(path)
        .map_err(|error| AudioReadError::Parse(error.to_string()))?
        .guess_file_type()
        .map_err(|error| AudioReadError::Parse(error.to_string()))?
        .read()
        .map_err(|error| AudioReadError::Parse(error.to_string()))?;
    let Some(tag) = tagged.primary_tag() else {
        return Ok(());
    };
    let mut id3v2 = Id3v2Tag::from(tag.clone());
    id3v2.remove_user_text("MusicBrainz Release Group Id");
    if let Some(release_group_id) = release_group_id {
        id3v2.insert_user_text(
            "MusicBrainz Release Group Id".to_owned(),
            release_group_id.to_owned(),
        );
    }
    id3v2
        .save_to_path(path, WriteOptions::new())
        .map_err(|error| AudioReadError::Write(error.to_string()))
}

#[cfg(test)]
mod tests;
