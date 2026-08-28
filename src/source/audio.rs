use std::fmt;
use std::fs::File;
use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::id3::v2::Id3v2Tag;
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::{Layer, MpegFile};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};

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
    pub artist_ids: Option<Vec<String>>,
    pub album_artist_ids: Option<Vec<String>>,
    pub compilation: bool,
    pub original_year: Option<u16>,
    pub track: u32,
    pub track_total: u32,
    pub disc: u32,
    pub disc_total: u32,
    pub recording_id: Option<String>,
    pub release_group_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioPreservationSnapshot {
    pub unrelated_items: Vec<PreservedTagItem>,
    pub pictures: Vec<PreservedPicture>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreservedTagItem {
    pub tag_type: String,
    pub key: String,
    pub value: PreservedTagValue,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreservedTagValue {
    Text(String),
    Locator(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservedPicture {
    pub tag_type: String,
    pub data: Vec<u8>,
    pub mime_type: Option<String>,
    pub picture_type: String,
    pub description: Option<String>,
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
        let span = tracing::trace_span!("probe_audio", path = %path.display());
        let _entered = span.enter();
        let probe = Probe::open(path)
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .guess_file_type()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?;
        let Some(file_type) = probe.file_type() else {
            return Ok(AudioProbe::NotAudio);
        };
        let format = match recognized_format(path, file_type)? {
            FormatRecognition::Supported(format) => format,
            FormatRecognition::Unsupported(description) => {
                return Ok(AudioProbe::Unsupported(description));
            }
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
        let exact_release_id = tagged
            .primary_tag()
            .and_then(|tag| tag.get_string(ItemKey::MusicBrainzReleaseId))
            .map(str::to_owned);
        let release_group_id = plan.release_group_id.clone().or_else(|| {
            tagged
                .primary_tag()
                .and_then(|tag| tag.get_string(ItemKey::MusicBrainzReleaseGroupId))
                .map(str::to_owned)
        });
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
            write_id3v2_album_ids(
                path,
                exact_release_id.as_deref(),
                release_group_id.as_deref(),
            )?;
        }
        Ok(())
    }

    pub fn preservation_snapshot(
        &self,
        path: &Path,
    ) -> Result<AudioPreservationSnapshot, AudioReadError> {
        let tagged = Probe::open(path)
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .guess_file_type()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?
            .read()
            .map_err(|error| AudioReadError::Parse(error.to_string()))?;
        let mut unrelated_items = Vec::new();
        let mut pictures = Vec::new();
        for tag in tagged.tags() {
            let tag_type = format!("{:?}", tag.tag_type());
            for item in tag.items().filter(|item| !groomed_key(&item.key())) {
                unrelated_items.push(PreservedTagItem {
                    tag_type: tag_type.clone(),
                    key: format!("{:?}", item.key()),
                    value: match item.value() {
                        ItemValue::Text(value) => PreservedTagValue::Text(value.clone()),
                        ItemValue::Locator(value) => PreservedTagValue::Locator(value.clone()),
                        ItemValue::Binary(value) => PreservedTagValue::Binary(value.clone()),
                    },
                });
            }
            for picture in tag.pictures() {
                pictures.push(PreservedPicture {
                    tag_type: tag_type.clone(),
                    data: picture.data().to_vec(),
                    mime_type: picture.mime_type().map(|mime| format!("{mime:?}")),
                    picture_type: format!("{:?}", picture.pic_type()),
                    description: picture.description().map(str::to_owned),
                });
            }
        }
        unrelated_items.sort();
        Ok(AudioPreservationSnapshot {
            unrelated_items,
            pictures,
        })
    }
}

fn groomed_key(key: &ItemKey) -> bool {
    matches!(
        key,
        ItemKey::TrackTitle
            | ItemKey::TrackArtist
            | ItemKey::TrackArtists
            | ItemKey::AlbumTitle
            | ItemKey::AlbumArtist
            | ItemKey::AlbumArtists
            | ItemKey::MusicBrainzArtistId
            | ItemKey::MusicBrainzReleaseArtistId
            | ItemKey::FlagCompilation
            | ItemKey::OriginalReleaseDate
            | ItemKey::RecordingDate
            | ItemKey::Year
            | ItemKey::TrackNumber
            | ItemKey::TrackTotal
            | ItemKey::DiscNumber
            | ItemKey::DiscTotal
            | ItemKey::MusicBrainzRecordingId
            | ItemKey::MusicBrainzReleaseGroupId
    )
}

enum FormatRecognition {
    Supported(AudioFormat),
    Unsupported(String),
}

fn recognized_format(
    path: &Path,
    file_type: FileType,
) -> Result<FormatRecognition, AudioReadError> {
    match file_type {
        FileType::Flac => Ok(FormatRecognition::Supported(AudioFormat::Flac)),
        FileType::Opus => Ok(FormatRecognition::Supported(AudioFormat::Opus)),
        FileType::Vorbis => Ok(FormatRecognition::Supported(AudioFormat::OggVorbis)),
        FileType::Mpeg => mpeg_format(path).map(|format| {
            format.map_or_else(
                || FormatRecognition::Unsupported("MPEG audio other than MP3".to_owned()),
                FormatRecognition::Supported,
            )
        }),
        FileType::Mp4 => mp4_format(path),
        _ => Ok(FormatRecognition::Unsupported(format!("{file_type:?}"))),
    }
}

fn mpeg_format(path: &Path) -> Result<Option<AudioFormat>, AudioReadError> {
    let mut file = File::open(path)?;
    let mpeg = MpegFile::read_from(&mut file, ParseOptions::new())
        .map_err(|error| AudioReadError::Parse(error.to_string()))?;
    Ok((*mpeg.properties().layer() == Layer::Layer3).then_some(AudioFormat::Mp3))
}

fn mp4_format(path: &Path) -> Result<FormatRecognition, AudioReadError> {
    let mut track_reader = File::open(path)?;
    if super::mp4::contains_video(&mut track_reader)
        .map_err(|error| AudioReadError::Parse(format!("cannot inspect MP4 tracks: {error}")))?
    {
        return Ok(FormatRecognition::Unsupported(
            "MP4 containing both audio and video".to_owned(),
        ));
    }
    let mut file = File::open(path)?;
    let mp4 = Mp4File::read_from(&mut file, ParseOptions::new())
        .map_err(|error| AudioReadError::Parse(error.to_string()))?;
    Ok(match mp4.properties().codec() {
        Some(Mp4Codec::AAC) => FormatRecognition::Supported(AudioFormat::M4aAac),
        Some(Mp4Codec::ALAC) => FormatRecognition::Supported(AudioFormat::M4aAlac),
        codec => FormatRecognition::Unsupported(format!("MP4 audio codec {codec:?}")),
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
        artist_ids: strings(tag, ItemKey::MusicBrainzArtistId),
        album_artist_ids: strings(tag, ItemKey::MusicBrainzReleaseArtistId),
        compilation: tag
            .get_string(ItemKey::FlagCompilation)
            .and_then(compilation_value),
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

fn compilation_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn apply_plan(tag: &mut Tag, plan: &PlannedTags) {
    tag.set_title(plan.title.clone());
    tag.set_artist(plan.artist.clone());
    replace_text_values(tag, ItemKey::TrackArtists, &plan.artists);
    tag.set_album(plan.album.clone());
    tag.insert_text(ItemKey::AlbumArtist, plan.album_artist.clone());
    replace_text_values(tag, ItemKey::AlbumArtists, &plan.album_artists);
    replace_optional_values(tag, ItemKey::MusicBrainzArtistId, &plan.artist_ids);
    replace_optional_values(
        tag,
        ItemKey::MusicBrainzReleaseArtistId,
        &plan.album_artist_ids,
    );
    tag.insert_text(
        ItemKey::FlagCompilation,
        if plan.compilation { "1" } else { "0" }.to_owned(),
    );
    if let Some(original_year) = plan.original_year {
        tag.insert_text(ItemKey::OriginalReleaseDate, original_year.to_string());
        tag.insert_text(ItemKey::RecordingDate, original_year.to_string());
    }
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
    let Some(value) = value else {
        return;
    };
    tag.take(key).for_each(drop);
    if key == ItemKey::MusicBrainzRecordingId {
        tag.insert_unchecked(TagItem::new(
            key,
            lofty::tag::ItemValue::Text(value.clone()),
        ));
    } else {
        tag.insert_text(key, value.clone());
    }
}

fn replace_optional_values(tag: &mut Tag, key: ItemKey, values: &Option<Vec<String>>) {
    if let Some(values) = values {
        replace_text_values(tag, key, values);
    }
}

fn write_id3v2_album_ids(
    path: &Path,
    exact_release_id: Option<&str>,
    release_group_id: Option<&str>,
) -> Result<(), AudioReadError> {
    if exact_release_id.is_none() && release_group_id.is_none() {
        return Ok(());
    }
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
    id3v2.remove_user_text("MusicBrainz Album Id");
    if let Some(exact_release_id) = exact_release_id {
        id3v2.insert_user_text(
            "MusicBrainz Album Id".to_owned(),
            exact_release_id.to_owned(),
        );
    }
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
