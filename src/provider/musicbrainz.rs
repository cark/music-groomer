use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::http::ProviderHttp;
use super::{
    MetadataProvider, ProviderError, ProviderProgress, ProviderSearch, ProviderSearchResult,
    collapse_equivalent,
};
use crate::domain::{Artist, ArtistCredit, CandidateRelease, Position, ReleaseKind, ReleaseTrack};

const API_ROOT: &str = "https://musicbrainz.org/ws/2";
const REQUEST_SPACING: Duration = Duration::from_secs(1);
const RETRY_DEADLINE: Duration = Duration::from_secs(60);
const DISCOVERY_GROUP_LIMIT: usize = 8;

trait MusicBrainzHttp {
    fn get_json<T: DeserializeOwned>(
        &mut self,
        url: &str,
        operation: &'static str,
        spacing: Duration,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError>;
}

impl MusicBrainzHttp for ProviderHttp {
    fn get_json<T: DeserializeOwned>(
        &mut self,
        url: &str,
        operation: &'static str,
        spacing: Duration,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError> {
        ProviderHttp::get_json(self, url, operation, spacing, deadline, progress)
    }
}

struct MusicBrainzClient<H> {
    http: H,
}

pub struct MusicBrainzProvider {
    client: MusicBrainzClient<ProviderHttp>,
}

impl MusicBrainzProvider {
    pub fn new() -> Self {
        Self {
            client: MusicBrainzClient {
                http: ProviderHttp::new(),
            },
        }
    }
}

impl Default for MusicBrainzProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: MusicBrainzHttp> MusicBrainzClient<H> {
    fn textual_search_url(
        search: &ProviderSearch,
        include_artist_id: bool,
    ) -> Result<String, ProviderError> {
        let title = if search.kind == crate::domain::SourceKind::LooseFile {
            search.title.as_ref().or(search.album.as_ref())
        } else {
            search.album.as_ref().or(search.title.as_ref())
        };
        let (Some(title), Some(artist)) = (title, search.artist.as_ref()) else {
            return Err(ProviderError::InsufficientEvidence);
        };
        let mut query = format!(
            "releasegroup:\"{}\" AND artist:\"{}\"",
            escaped(title),
            escaped(artist)
        );
        if include_artist_id
            && let Some(artist_id) = search
                .album_artist_ids
                .first()
                .or(search.artist_ids.first())
        {
            query.push_str(&format!(" AND arid:{artist_id}"));
        }
        Ok(format!(
            "{API_ROOT}/release-group/?query={}&fmt=json&limit={DISCOVERY_GROUP_LIMIT}",
            urlencoding::encode(&query)
        ))
    }

    fn browse_group_url(group_id: &str) -> String {
        format!(
            "{API_ROOT}/release?release-group={group_id}&inc=artist-credits+recordings+release-groups&fmt=json&limit=100"
        )
    }

    fn browse_recording_url(recording_id: &str) -> String {
        format!(
            "{API_ROOT}/release?recording={recording_id}&inc=artist-credits+recordings+release-groups&fmt=json&limit=100"
        )
    }

    fn request<T: DeserializeOwned>(
        &mut self,
        url: &str,
        operation: &'static str,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError> {
        self.http
            .get_json(url, operation, REQUEST_SPACING, deadline, progress)
    }

    fn discover(
        &mut self,
        search: &ProviderSearch,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<(Vec<String>, Vec<String>), ProviderError> {
        let mut group_ids = Vec::new();
        let mut warnings = Vec::new();

        if let Some(group_id) = &search.release_group_id {
            group_ids.push(group_id.clone());
        } else if !search.recording_ids.is_empty() {
            for recording_id in &search.recording_ids {
                let browse: ReleaseBrowse = self.request(
                    &Self::browse_recording_url(recording_id),
                    "MusicBrainz recording releases",
                    deadline,
                    progress,
                )?;
                for release in browse
                    .releases
                    .into_iter()
                    .filter(ReleaseDetail::is_official)
                {
                    let Some(group_id) =
                        release.release_group.as_ref().map(|group| group.id.clone())
                    else {
                        continue;
                    };
                    if !group_ids.contains(&group_id) && group_ids.len() < DISCOVERY_GROUP_LIMIT {
                        group_ids.push(group_id.clone());
                    }
                }
                if group_ids.len() >= DISCOVERY_GROUP_LIMIT {
                    break;
                }
            }
            if group_ids.is_empty() {
                warnings.push(
                    "Existing MusicBrainz recording identifiers found no releases; falling back to artist and title search"
                        .into(),
                );
            }
        }

        if group_ids.is_empty() {
            let found: ReleaseGroupSearch = self.request(
                &Self::textual_search_url(search, true)?,
                "MusicBrainz release-group search",
                deadline,
                progress,
            )?;
            group_ids.extend(
                found
                    .release_groups
                    .into_iter()
                    .map(|group| group.id)
                    .take(DISCOVERY_GROUP_LIMIT),
            );
            if group_ids.is_empty()
                && (!search.album_artist_ids.is_empty() || !search.artist_ids.is_empty())
            {
                warnings.push(
                    "Existing MusicBrainz artist identifiers found no release groups; falling back to artist names"
                        .into(),
                );
                let found: ReleaseGroupSearch = self.request(
                    &Self::textual_search_url(search, false)?,
                    "MusicBrainz release-group search",
                    deadline,
                    progress,
                )?;
                group_ids.extend(
                    found
                        .release_groups
                        .into_iter()
                        .map(|group| group.id)
                        .take(DISCOVERY_GROUP_LIMIT),
                );
            }
            if group_ids.is_empty()
                && search.kind == crate::domain::SourceKind::LooseFile
                && search.album.is_some()
                && search.title.is_some()
                && search.album != search.title
            {
                let mut album_fallback = search.clone();
                album_fallback.title = None;
                let found: ReleaseGroupSearch = self.request(
                    &Self::textual_search_url(&album_fallback, false)?,
                    "MusicBrainz album fallback search",
                    deadline,
                    progress,
                )?;
                group_ids.extend(
                    found
                        .release_groups
                        .into_iter()
                        .map(|group| group.id)
                        .take(DISCOVERY_GROUP_LIMIT),
                );
            }
        }
        Ok((group_ids, warnings))
    }
}

impl<H: MusicBrainzHttp> MusicBrainzClient<H> {
    fn search(
        &mut self,
        search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError> {
        if !search.is_usable() {
            return Err(ProviderError::InsufficientEvidence);
        }
        let deadline = Instant::now() + RETRY_DEADLINE;
        let (group_ids, mut warnings) = self.discover(search, deadline, progress)?;
        let identifier_discovery =
            search.release_group_id.is_some() || !search.recording_ids.is_empty();
        let mut candidates = Vec::new();

        for group_id in group_ids {
            let browse: ReleaseBrowse = self.request(
                &Self::browse_group_url(&group_id),
                "MusicBrainz release variants",
                deadline,
                progress,
            )?;
            let releases = browse
                .releases
                .into_iter()
                .filter(ReleaseDetail::is_official);
            let compatible = releases.into_iter().filter(|release| {
                search.kind == crate::domain::SourceKind::LooseFile
                    || release.track_count() == search.track_count
            });
            candidates.extend(compatible.filter_map(ReleaseDetail::into_candidate));
        }

        if identifier_discovery && candidates.is_empty() {
            warnings.push(if search.release_group_id.is_some() {
                "Existing MusicBrainz release-group identifier found no compatible official release; falling back to artist and title search".into()
            } else {
                "Existing MusicBrainz recording identifiers found no compatible official release; falling back to artist and title search".into()
            });
            let mut fallback = search.clone();
            fallback.release_group_id = None;
            fallback.recording_ids.clear();
            if fallback.artist.is_some() && (fallback.album.is_some() || fallback.title.is_some()) {
                let (groups, _) = self.discover(&fallback, deadline, progress)?;
                for group_id in groups {
                    let browse: ReleaseBrowse = self.request(
                        &Self::browse_group_url(&group_id),
                        "MusicBrainz release variants",
                        deadline,
                        progress,
                    )?;
                    candidates.extend(
                        browse
                            .releases
                            .into_iter()
                            .filter(ReleaseDetail::is_official)
                            .filter(|release| {
                                search.kind == crate::domain::SourceKind::LooseFile
                                    || release.track_count() == search.track_count
                            })
                            .filter_map(ReleaseDetail::into_candidate),
                    );
                }
            }
        }

        Ok(ProviderSearchResult {
            candidates: collapse_equivalent(candidates),
            warnings,
        })
    }
}

impl MetadataProvider for MusicBrainzProvider {
    fn search(
        &mut self,
        search: &ProviderSearch,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ProviderSearchResult, ProviderError> {
        self.client.search(search, progress)
    }
}

fn escaped(value: &str) -> String {
    value.replace(['\\', '"'], " ")
}

#[derive(Deserialize)]
struct ReleaseGroupSearch {
    #[serde(rename = "release-groups", default)]
    release_groups: Vec<ReleaseGroupHit>,
}

#[derive(Deserialize)]
struct ReleaseGroupHit {
    id: String,
}

#[derive(Deserialize)]
struct ReleaseBrowse {
    #[serde(default)]
    releases: Vec<ReleaseDetail>,
}

#[derive(Deserialize)]
struct ReleaseDetail {
    id: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
    #[serde(default)]
    media: Vec<Medium>,
}

impl ReleaseDetail {
    fn is_official(&self) -> bool {
        self.status.eq_ignore_ascii_case("official")
    }

    fn track_count(&self) -> usize {
        self.media.iter().map(|medium| medium.tracks.len()).sum()
    }

    fn into_candidate(self) -> Option<CandidateRelease> {
        let group = self.release_group?;
        let album_artist = artist_credit(&group.artist_credit)?;
        let kind = release_kind(&group, &album_artist);
        let mut tracks = Vec::new();
        for (medium_index, medium) in self.media.into_iter().enumerate() {
            let disc = u16::try_from(medium.position.unwrap_or(medium_index + 1)).ok()?;
            for (track_index, track) in medium.tracks.into_iter().enumerate() {
                let position = u16::try_from(track.position.unwrap_or(track_index + 1)).ok()?;
                let recording_credit = artist_credit(&track.recording.artist_credit);
                tracks.push(ReleaseTrack {
                    title: track.title.unwrap_or(track.recording.title),
                    artist_credit: artist_credit(&track.artist_credit)
                        .or(recording_credit)
                        .unwrap_or_else(|| album_artist.clone()),
                    position: Position::new(disc, position),
                    duration_ms: track.length.or(track.recording.length).unwrap_or(0),
                    recording_id: Some(track.recording.id),
                });
            }
        }
        (!tracks.is_empty()).then_some(CandidateRelease {
            provider_key: self.id,
            title: group.title,
            album_artist,
            original_year: year(group.first_release_date.as_deref()),
            kind,
            tracks,
            release_group_id: Some(group.id),
            exact_release_id: None,
        })
    }
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<CreditPart>,
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
    title: Option<String>,
    length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<CreditPart>,
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

fn release_kind(group: &ReleaseGroup, credit: &ArtistCredit) -> ReleaseKind {
    if is_various_artists(credit) {
        ReleaseKind::Compilation
    } else if group.primary_type.eq_ignore_ascii_case("album") {
        ReleaseKind::Album
    } else if group.primary_type.eq_ignore_ascii_case("single") {
        ReleaseKind::Single
    } else if group.primary_type.eq_ignore_ascii_case("ep") {
        ReleaseKind::Ep
    } else {
        ReleaseKind::Other(group.primary_type.clone())
    }
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
    use serde_json::Value;
    use std::collections::VecDeque;

    struct FakeHttp {
        responses: VecDeque<Value>,
        urls: Vec<String>,
    }

    impl MusicBrainzHttp for FakeHttp {
        fn get_json<T: DeserializeOwned>(
            &mut self,
            url: &str,
            _operation: &'static str,
            _spacing: Duration,
            _deadline: Instant,
            _progress: &mut dyn ProviderProgress,
        ) -> Result<T, ProviderError> {
            self.urls.push(url.to_owned());
            let value = self.responses.pop_front().expect("unexpected request");
            serde_json::from_value(value)
                .map_err(|error| ProviderError::InvalidResponse(error.to_string()))
        }
    }

    #[test]
    fn browses_capped_groups_and_keeps_materially_distinct_official_variants() {
        let groups = serde_json::json!({
            "release-groups": (0..10).map(|index| serde_json::json!({"id": format!("group-{index}")})).collect::<Vec<_>>()
        });
        let browse = |id: usize| serde_json::json!({"releases": [release(id, "Track", "Official"), release(id + 20, "Different", "Official"), release(id + 40, "Unofficial", "Bootleg")]});
        let mut responses = VecDeque::from([groups]);
        responses.extend((0..DISCOVERY_GROUP_LIMIT).map(browse));
        let mut provider = MusicBrainzClient {
            http: FakeHttp {
                responses,
                urls: Vec::new(),
            },
        };

        let result = provider.search(&search(), &mut ()).unwrap();

        assert_eq!(provider.http.urls.len(), 1 + DISCOVERY_GROUP_LIMIT);
        assert_eq!(result.candidates.len(), DISCOVERY_GROUP_LIMIT * 2);
        assert!(provider.http.urls[0].contains("limit=8"));
    }

    #[test]
    fn maps_release_group_identity_and_prefers_release_track_credit_and_title() {
        let detail: ReleaseDetail = serde_json::from_str(include_str!(
            "../../tests/fixtures/musicbrainz/release-detail.json"
        ))
        .expect("fixture should parse");

        let candidate = detail.into_candidate().expect("fixture should map");

        assert_eq!(candidate.title, "Original Album Title");
        assert_eq!(candidate.album_artist.display, "Alice with Bob");
        assert_eq!(candidate.original_year, Some(1974));
        assert_eq!(candidate.tracks[0].title, "Opening (release version)");
        assert_eq!(candidate.tracks[0].artist_credit.display, "Alice feat. Bob");
        assert_eq!(candidate.exact_release_id, None);
    }

    #[test]
    fn recording_identifier_uses_browse_instead_of_an_invalid_search_field() {
        let mut search = search();
        search.recording_ids = vec!["recording-id".into()];
        let mut provider = MusicBrainzClient {
            http: FakeHttp {
                responses: VecDeque::from([
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                ]),
                urls: Vec::new(),
            },
        };

        provider.search(&search, &mut ()).unwrap();

        assert!(provider.http.urls[0].contains("recording=recording-id"));
        assert!(!provider.http.urls[0].contains("rid%3A"));
        assert!(provider.http.urls[1].contains("release-group=group-1"));
    }

    #[test]
    fn stale_artist_identifier_warns_and_falls_back_to_name_search() {
        let mut provider = MusicBrainzClient {
            http: FakeHttp {
                responses: VecDeque::from([
                    serde_json::json!({"release-groups": []}),
                    serde_json::json!({"release-groups": [{"id": "group-1"}]}),
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                ]),
                urls: Vec::new(),
            },
        };

        let result = provider.search(&search(), &mut ()).unwrap();

        assert!(provider.http.urls[0].contains("arid%3Aartist-id"));
        assert!(!provider.http.urls[1].contains("arid%3Aartist-id"));
        assert_eq!(result.candidates.len(), 1);
        assert!(result.warnings[0].contains("falling back to artist names"));
    }

    #[test]
    fn stale_release_group_identifier_warns_and_falls_back_to_text_search() {
        let mut search = search();
        search.release_group_id = Some("stale-group".into());
        let mut provider = MusicBrainzClient {
            http: FakeHttp {
                responses: VecDeque::from([
                    serde_json::json!({"releases": []}),
                    serde_json::json!({"release-groups": [{"id": "group-1"}]}),
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                ]),
                urls: Vec::new(),
            },
        };

        let result = provider.search(&search, &mut ()).unwrap();

        assert!(provider.http.urls[0].contains("release-group=stale-group"));
        assert!(provider.http.urls[1].contains("release-group/?query="));
        assert_eq!(result.candidates.len(), 1);
        assert!(result.warnings[0].contains("release-group identifier"));
    }

    #[test]
    fn incompatible_recording_identifier_result_falls_back_to_text_search() {
        let mut search = search();
        search.track_count = 2;
        search.recording_ids = vec!["stale-recording".into()];
        let mut provider = MusicBrainzClient {
            http: FakeHttp {
                responses: VecDeque::from([
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                    serde_json::json!({"releases": [release(1, "Track", "Official")]}),
                    serde_json::json!({"release-groups": [{"id": "group-2"}]}),
                    serde_json::json!({"releases": [two_track_release()]}),
                ]),
                urls: Vec::new(),
            },
        };

        let result = provider.search(&search, &mut ()).unwrap();

        assert_eq!(result.candidates.len(), 1);
        assert!(result.warnings.iter().any(|warning| {
            warning.contains("recording identifiers found no compatible official release")
        }));
    }

    #[test]
    fn maps_ep_and_unusual_primary_type_without_calling_them_albums() {
        let ep = serde_json::from_value::<ReleaseDetail>(release_with_type("EP")).unwrap();
        let other =
            serde_json::from_value::<ReleaseDetail>(release_with_type("Broadcast")).unwrap();

        assert_eq!(ep.into_candidate().unwrap().kind, ReleaseKind::Ep);
        assert_eq!(
            other.into_candidate().unwrap().kind,
            ReleaseKind::Other("Broadcast".into())
        );
    }

    #[test]
    fn loose_text_search_tries_the_track_title_before_an_existing_album_name() {
        let mut search = search();
        search.kind = crate::domain::SourceKind::LooseFile;
        search.title = Some("Song".into());
        search.album = Some("Album".into());

        let url = MusicBrainzClient::<FakeHttp>::textual_search_url(&search, false).unwrap();

        assert!(url.contains("Song"));
        assert!(!url.contains("Album"));
    }

    fn search() -> ProviderSearch {
        ProviderSearch {
            kind: crate::domain::SourceKind::AlbumDirectory,
            album: Some("Album".into()),
            artist: Some("Artist".into()),
            artist_ids: Vec::new(),
            album_artist_ids: vec!["artist-id".into()],
            title: None,
            release_group_id: None,
            recording_ids: Vec::new(),
            track_count: 1,
        }
    }

    fn release(id: usize, track_title: &str, status: &str) -> Value {
        serde_json::json!({
            "id": format!("release-{id}"), "status": status,
            "release-group": {"id": format!("group-{id}"), "title": "Album", "artist-credit": credit(), "first-release-date": "1974", "primary-type": "Album"},
            "media": [{"position": 1, "tracks": [{"position": 1, "title": track_title, "length": 120000, "artist-credit": credit(), "recording": {"id": format!("recording-{id}"), "title": "Recording", "length": 120000, "artist-credit": credit()}}]}]
        })
    }

    fn release_with_type(primary_type: &str) -> Value {
        let mut value = release(1, "Track", "Official");
        value["release-group"]["primary-type"] = Value::String(primary_type.into());
        value
    }

    fn two_track_release() -> Value {
        let mut value = release(2, "First", "Official");
        let second = serde_json::json!({
            "position": 2,
            "title": "Second",
            "length": 130000,
            "artist-credit": credit(),
            "recording": {"id": "recording-second", "title": "Second", "length": 130000, "artist-credit": credit()}
        });
        value["media"][0]["tracks"]
            .as_array_mut()
            .unwrap()
            .push(second);
        value
    }

    fn credit() -> Value {
        serde_json::json!([{"name": "Artist", "artist": {"id": "artist-id", "name": "Artist"}}])
    }
}
