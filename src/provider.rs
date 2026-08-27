mod acoustid;
mod acoustid_lookup;
mod artwork_lookup;
mod cache;
mod cover_art_archive;
mod http;
mod lookup;
mod musicbrainz;
mod search;

pub use acoustid::{
    ACOUSTID_USABLE_SCORE, AcoustId, AcoustIdProvider, AcoustIdResponse, AcoustIdResult,
};
pub use acoustid_lookup::{AcoustIdLookup, AcoustIdLookupOrigin, AcoustIdResolver};
pub use artwork_lookup::{ArtworkLookup, ArtworkLookupOrigin, ArtworkResolver};
pub use cache::{
    AcoustIdCacheEntry, ArtworkCacheEntry, CacheError, CacheStatus, MetadataCacheEntry,
    MetadataFreshness, ProviderCache,
};
pub use cover_art_archive::{ArtworkProvider, CoverArtArchive, ProviderArtwork};
pub use lookup::{LookupOrigin, MetadataLookup, MetadataResolver};
pub use musicbrainz::MusicBrainzProvider;
pub use search::{
    MetadataProvider, ProviderError, ProviderEvent, ProviderName, ProviderProgress, ProviderSearch,
    ProviderSearchResult, WaitReason, source_inspection,
};

use crate::domain::CandidateRelease;

pub const METADATA_FRESH_DAYS: u64 = 30;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

pub fn collapse_equivalent(candidates: Vec<CandidateRelease>) -> Vec<CandidateRelease> {
    let mut distinct = Vec::new();
    for candidate in candidates {
        if !distinct
            .iter()
            .any(|existing| equivalent_groomed_result(existing, &candidate))
        {
            distinct.push(candidate);
        }
    }
    distinct
}

pub fn equivalent_groomed_result(left: &CandidateRelease, right: &CandidateRelease) -> bool {
    left.title == right.title
        && left.album_artist == right.album_artist
        && left.original_year == right.original_year
        && left.kind == right.kind
        && left.tracks.len() == right.tracks.len()
        && left.tracks.iter().zip(&right.tracks).all(|(left, right)| {
            left.title == right.title
                && left.artist_credit == right.artist_credit
                && left.position == right.position
                && left.recording_id == right.recording_id
        })
        && left.release_group_id == right.release_group_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtistCredit, Position, ReleaseKind, ReleaseTrack};

    fn candidate(release: &str) -> CandidateRelease {
        CandidateRelease {
            provider_key: release.into(),
            title: "Album".into(),
            album_artist: ArtistCredit::single("Artist"),
            original_year: Some(1970),
            kind: ReleaseKind::Album,
            tracks: vec![ReleaseTrack {
                title: "Track".into(),
                artist_credit: ArtistCredit::single("Artist"),
                position: Position::new(1, 1),
                duration_ms: 120_000,
                recording_id: Some("recording".into()),
            }],
            release_group_id: Some("group".into()),
            exact_release_id: Some(release.into()),
        }
    }

    #[test]
    fn collapses_editions_that_would_groom_identically() {
        let candidates = collapse_equivalent(vec![candidate("release-a"), candidate("release-b")]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].provider_key, "release-a");
    }

    #[test]
    fn keeps_materially_different_track_lists() {
        let first = candidate("release-a");
        let mut second = candidate("release-b");
        second.tracks[0].title = "Different".into();

        assert_eq!(collapse_equivalent(vec![first, second]).len(), 2);
    }
}
