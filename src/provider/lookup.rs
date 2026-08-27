use std::time::SystemTime;

use super::{
    CacheError, MetadataFreshness, MetadataProvider, ProviderCache, ProviderProgress,
    ProviderSearch,
};
use crate::domain::CandidateRelease;

pub struct MetadataResolver<P> {
    provider: P,
    cache: ProviderCache,
}

impl<P: MetadataProvider> MetadataResolver<P> {
    pub fn new(provider: P, cache: ProviderCache) -> Self {
        Self { provider, cache }
    }

    pub fn lookup(
        &mut self,
        search: &ProviderSearch,
        offline: bool,
        force_refresh: bool,
        now: SystemTime,
        progress: &mut dyn ProviderProgress,
    ) -> MetadataLookup {
        let mut warnings = Vec::new();
        let cached = match self.cache.metadata(search, now) {
            Ok(cached) => {
                if let Some(warning) = cached
                    .as_ref()
                    .and_then(|entry| entry.maintenance_warning.as_ref())
                {
                    warnings.push(format!("Could not maintain provider cache: {warning}"));
                }
                cached
            }
            Err(CacheError::Damaged(path, error)) => {
                warnings.push(format!(
                    "Ignored damaged cache entry {}: {error}",
                    path.display()
                ));
                None
            }
            Err(error) => {
                warnings.push(format!("Could not read provider cache: {error}"));
                None
            }
        };

        if offline {
            return match cached {
                Some(entry) => MetadataLookup {
                    candidates: entry.candidates,
                    origin: match entry.freshness {
                        MetadataFreshness::Fresh => LookupOrigin::FreshCache,
                        MetadataFreshness::Stale => LookupOrigin::OfflineStaleCache,
                    },
                    warnings,
                },
                None => MetadataLookup {
                    candidates: Vec::new(),
                    origin: LookupOrigin::OfflineMiss,
                    warnings,
                },
            };
        }

        if !force_refresh
            && let Some(entry) = cached.as_ref()
            && entry.freshness == MetadataFreshness::Fresh
        {
            return MetadataLookup {
                candidates: entry.candidates.clone(),
                origin: LookupOrigin::FreshCache,
                warnings,
            };
        }

        match self.provider.search(search, progress) {
            Ok(candidates) => {
                if let Err(error) = self.cache.store_metadata(search, &candidates, now) {
                    warnings.push(format!("Could not update provider cache: {error}"));
                }
                MetadataLookup {
                    candidates,
                    origin: if force_refresh {
                        LookupOrigin::Refreshed
                    } else {
                        LookupOrigin::Live
                    },
                    warnings,
                }
            }
            Err(error) => match cached {
                Some(entry) => {
                    warnings.push(format!(
                        "MusicBrainz could not be refreshed ({error}); using stale cached data"
                    ));
                    MetadataLookup {
                        candidates: entry.candidates,
                        origin: LookupOrigin::StaleFallback,
                        warnings,
                    }
                }
                None => {
                    warnings.push(format!(
                        "MusicBrainz is unavailable and no cached result exists: {error}"
                    ));
                    MetadataLookup {
                        candidates: Vec::new(),
                        origin: LookupOrigin::ProviderUnavailable,
                        warnings,
                    }
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataLookup {
    pub candidates: Vec<CandidateRelease>,
    pub origin: LookupOrigin,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupOrigin {
    Live,
    Refreshed,
    FreshCache,
    StaleFallback,
    OfflineStaleCache,
    OfflineMiss,
    ProviderUnavailable,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use tempfile::TempDir;

    use super::*;
    use crate::domain::{ArtistCredit, ReleaseKind, SourceKind};
    use crate::provider::ProviderError;

    struct FakeProvider {
        calls: usize,
        result: Result<Vec<CandidateRelease>, ProviderError>,
    }

    impl MetadataProvider for FakeProvider {
        fn search(
            &mut self,
            _search: &ProviderSearch,
            _progress: &mut dyn ProviderProgress,
        ) -> Result<Vec<CandidateRelease>, ProviderError> {
            self.calls += 1;
            std::mem::replace(
                &mut self.result,
                Err(ProviderError::Network("fake result consumed".into())),
            )
        }
    }

    #[test]
    fn fresh_cache_completely_bypasses_provider() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_metadata(&search(), &[candidate("cached")], UNIX_EPOCH)
            .unwrap();
        let provider = FakeProvider {
            calls: 0,
            result: Ok(vec![candidate("live")]),
        };
        let mut resolver = MetadataResolver::new(provider, cache);

        let result = resolver.lookup(&search(), false, false, UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, LookupOrigin::FreshCache);
        assert_eq!(result.candidates[0].provider_key, "cached");
        assert_eq!(resolver.provider.calls, 0);
    }

    #[test]
    fn failed_refresh_keeps_stale_data_and_explains_it() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_metadata(&search(), &[candidate("stale")], UNIX_EPOCH)
            .unwrap();
        let provider = FakeProvider {
            calls: 0,
            result: Err(ProviderError::Network("offline".into())),
        };
        let mut resolver = MetadataResolver::new(provider, cache);
        let now = UNIX_EPOCH + Duration::from_secs(31 * 86_400);

        let result = resolver.lookup(&search(), false, false, now, &mut ());

        assert_eq!(result.origin, LookupOrigin::StaleFallback);
        assert_eq!(result.candidates[0].provider_key, "stale");
        assert!(result.warnings[0].contains("using stale cached data"));
    }

    #[test]
    fn offline_mode_uses_stale_cache_without_provider_call() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_metadata(&search(), &[candidate("stale")], UNIX_EPOCH)
            .unwrap();
        let provider = FakeProvider {
            calls: 0,
            result: Ok(vec![candidate("live")]),
        };
        let mut resolver = MetadataResolver::new(provider, cache);
        let now = UNIX_EPOCH + Duration::from_secs(31 * 86_400);

        let result = resolver.lookup(&search(), true, false, now, &mut ());

        assert_eq!(result.origin, LookupOrigin::OfflineStaleCache);
        assert_eq!(resolver.provider.calls, 0);
    }

    fn search() -> ProviderSearch {
        ProviderSearch {
            kind: SourceKind::AlbumDirectory,
            album: Some("Album".into()),
            artist: Some("Artist".into()),
            title: None,
            release_group_id: None,
            recording_ids: Vec::new(),
            track_count: 1,
        }
    }

    fn candidate(key: &str) -> CandidateRelease {
        CandidateRelease {
            provider_key: key.into(),
            title: "Album".into(),
            album_artist: ArtistCredit::single("Artist"),
            original_year: Some(2000),
            kind: ReleaseKind::Album,
            tracks: Vec::new(),
            release_group_id: Some("group".into()),
            exact_release_id: None,
        }
    }
}
