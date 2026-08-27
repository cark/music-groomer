use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::http::ProviderHttp;
use super::{
    MetadataProvider, ProviderError, ProviderProgress, ProviderSearch, collapse_equivalent,
};
use crate::domain::{Artist, ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack};

const API_ROOT: &str = "https://musicbrainz.org/ws/2";
const REQUEST_SPACING: Duration = Duration::from_secs(1);
const RETRY_DEADLINE: Duration = Duration::from_secs(60);

pub struct MusicBrainzProvider {
    http: ProviderHttp,
}

impl MusicBrainzProvider {
    pub fn new() -> Self {
        Self {
            http: ProviderHttp::new(),
        }
    }

    fn search_url(search: &ProviderSearch) -> Result<String, ProviderError> {
        let query = if let Some(group) = &search.release_group_id {
            format!("rgid:{group}")
        } else if let Some(recording) = search.recording_ids.first() {
            format!("rid:{recording}")
        } else if let (Some(album), Some(artist)) = (&search.album, &search.artist) {
            format!(
                "release:\"{}\" AND artist:\"{}\"",
                escaped(album),
                escaped(artist)
            )
        } else if let (Some(title), Some(artist)) = (&search.title, &search.artist) {
            format!(
                "recording:\"{}\" AND artist:\"{}\"",
                escaped(title),
                escaped(artist)
            )
        } else {
            return Err(ProviderError::InsufficientEvidence);
        };
        let query = format!("({query}) AND status:official");
        Ok(format!(
            "{API_ROOT}/release/?query={}&fmt=json&limit=25",
            urlencoding::encode(&query)
        ))
    }
}

impl Default for MusicBrainzProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataProvider for MusicBrainzProvider {
    fn search(
        &mut self,
        search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<Vec<CandidateRelease>, ProviderError> {
        if !search.is_usable() {
            return Err(ProviderError::InsufficientEvidence);
        }
        let deadline = Instant::now() + RETRY_DEADLINE;
        let found: ReleaseSearch = self.http.get_json(
            &Self::search_url(search)?,
            "MusicBrainz search",
            REQUEST_SPACING,
            deadline,
            progress,
        )?;
        let likely = found.releases.into_iter().filter(|release| {
            release.media.is_empty()
                || release
                    .media
                    .iter()
                    .map(|medium| medium.track_count)
                    .sum::<usize>()
                    == search.track_count
                || search.kind == crate::domain::SourceKind::LooseFile
        });
        let mut candidates = Vec::new();
        let mut seen_groups = BTreeSet::new();
        for release in likely.filter(|release| {
            seen_groups.insert(
                release
                    .release_group
                    .as_ref()
                    .map_or_else(|| release.id.clone(), |group| group.id.clone()),
            )
        }) {
            let url = format!(
                "{API_ROOT}/release/{}?inc=artist-credits+recordings+release-groups&fmt=json",
                release.id
            );
            let detail: ReleaseDetail = self.http.get_json(
                &url,
                "MusicBrainz release details",
                REQUEST_SPACING,
                deadline,
                progress,
            )?;
            if let Some(candidate) = detail.into_candidate() {
                candidates.push(candidate);
            }
        }
        Ok(collapse_equivalent(candidates))
    }
}

fn escaped(value: &str) -> String {
    value.replace(['\\', '"'], " ")
}

#[derive(Deserialize)]
struct ReleaseSearch {
    #[serde(default)]
    releases: Vec<ReleaseHit>,
}

#[derive(Deserialize)]
struct ReleaseHit {
    id: String,
    #[serde(rename = "release-group")]
    release_group: Option<SearchReleaseGroup>,
    #[serde(default)]
    media: Vec<SearchMedium>,
}

#[derive(Deserialize)]
struct SearchReleaseGroup {
    id: String,
}

#[derive(Deserialize)]
struct SearchMedium {
    #[serde(rename = "track-count", default)]
    track_count: usize,
}

#[derive(Deserialize)]
struct ReleaseDetail {
    id: String,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<CreditPart>,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
    #[serde(default)]
    media: Vec<Medium>,
}

impl ReleaseDetail {
    fn into_candidate(self) -> Option<CandidateRelease> {
        let group = self.release_group?;
        let album_artist = artist_credit(&self.artist_credit)?;
        let kind = if is_various_artists(&album_artist) {
            ReleaseKind::Compilation
        } else if group.primary_type.eq_ignore_ascii_case("single") {
            ReleaseKind::Single
        } else {
            ReleaseKind::Album
        };
        let mut tracks = Vec::new();
        for (medium_index, medium) in self.media.into_iter().enumerate() {
            let disc = u16::try_from(medium.position.unwrap_or(medium_index + 1)).ok()?;
            for (track_index, track) in medium.tracks.into_iter().enumerate() {
                let position = u16::try_from(track.position.unwrap_or(track_index + 1)).ok()?;
                let recording = track.recording;
                tracks.push(ReleaseTrack {
                    title: recording.title,
                    artist_credit: artist_credit(&recording.artist_credit)
                        .unwrap_or_else(|| album_artist.clone()),
                    position: Position::new(disc, position),
                    duration_ms: track.length.or(recording.length).unwrap_or(0),
                    recording_id: Some(recording.id),
                });
            }
        }
        (!tracks.is_empty()).then_some(CandidateRelease {
            provider_key: self.id.clone(),
            title: group.title.unwrap_or(self.title),
            album_artist,
            original_year: year(group.first_release_date.as_deref()),
            kind,
            tracks,
            release_group_id: Some(group.id),
            exact_release_id: Some(self.id),
        })
    }
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
    #[serde(rename = "primary-type", default)]
    primary_type: String,
}

#[derive(Deserialize)]
struct Medium {
    position: Option<usize>,
    #[serde(default)]
    tracks: Vec<Track>,
}

#[derive(Deserialize)]
struct Track {
    position: Option<usize>,
    length: Option<u64>,
    recording: Recording,
}

#[derive(Deserialize)]
struct Recording {
    id: String,
    title: String,
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<CreditPart>,
}

#[derive(Deserialize)]
struct CreditPart {
    name: String,
    #[serde(default)]
    joinphrase: String,
    artist: CreditArtist,
}

#[derive(Deserialize)]
struct CreditArtist {
    id: String,
    name: String,
}

fn artist_credit(parts: &[CreditPart]) -> Option<ArtistCredit> {
    (!parts.is_empty()).then(|| ArtistCredit {
        display: parts
            .iter()
            .map(|part| format!("{}{}", part.name, part.joinphrase))
            .collect(),
        artists: parts
            .iter()
            .map(|part| Artist {
                name: part.artist.name.clone(),
                musicbrainz_id: Some(part.artist.id.clone()),
            })
            .collect(),
    })
}

fn year(value: Option<&str>) -> Option<u16> {
    value?.get(..4)?.parse().ok()
}

fn is_various_artists(credit: &ArtistCredit) -> bool {
    const VARIOUS_ARTISTS_ID: &str = "89ad4ac3-39f7-470e-963a-56509c546377";
    credit.artists.len() == 1
        && credit.artists[0].musicbrainz_id.as_deref() == Some(VARIOUS_ARTISTS_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_collaboration_credit_and_original_release_year() {
        let detail: ReleaseDetail = serde_json::from_str(include_str!(
            "../../tests/fixtures/musicbrainz/release-detail.json"
        ))
        .expect("fixture should parse");

        let candidate = detail.into_candidate().expect("fixture should map");

        assert_eq!(candidate.album_artist.display, "Alice with Bob");
        assert_eq!(candidate.album_artist.artists.len(), 2);
        assert_eq!(candidate.original_year, Some(1974));
        assert_eq!(candidate.tracks[0].artist_credit.display, "Alice & Bob");
        assert_eq!(candidate.exact_release_id.as_deref(), Some("release-1"));
    }

    #[test]
    fn search_url_prefers_existing_identifier() {
        let search = ProviderSearch {
            kind: crate::domain::SourceKind::AlbumDirectory,
            album: Some("Ignored".into()),
            artist: Some("Ignored".into()),
            title: None,
            release_group_id: Some("group-id".into()),
            recording_ids: Vec::new(),
            track_count: 1,
        };

        assert!(
            MusicBrainzProvider::search_url(&search)
                .unwrap()
                .contains("rgid%3Agroup-id")
        );
    }

    #[test]
    fn compilation_requires_the_various_artists_identity() {
        let named_artist = ArtistCredit::credited(
            "One Artist",
            vec![Artist {
                name: "One Artist".into(),
                musicbrainz_id: Some("artist-id".into()),
            }],
        );
        let various_artists = ArtistCredit::credited(
            "Various Artists",
            vec![Artist {
                name: "Various Artists".into(),
                musicbrainz_id: Some("89ad4ac3-39f7-470e-963a-56509c546377".into()),
            }],
        );

        assert!(!is_various_artists(&named_artist));
        assert!(is_various_artists(&various_artists));
    }
}
