use std::time::SystemTime;

use super::{
    AcoustIdProvider, AcoustIdResponse, CacheError, MetadataFreshness, ProviderCache,
    ProviderProgress,
};
use crate::fingerprint::AudioFingerprint;

pub struct AcoustIdResolver<P> {
    provider: P,
    cache: ProviderCache,
}

impl<P: AcoustIdProvider> AcoustIdResolver<P> {
    pub fn new(provider: P, cache: ProviderCache) -> Self {
        Self { provider, cache }
    }

    pub fn lookup(
        &mut self,
        fingerprint: &AudioFingerprint,
        offline: bool,
        force_refresh: bool,
        now: SystemTime,
        progress: &mut dyn ProviderProgress,
    ) -> AcoustIdLookup {
        let mut warnings = Vec::new();
        let cached = match self.cache.acoustid(fingerprint, now) {
            Ok(cached) => {
                if let Some(warning) = cached
                    .as_ref()
                    .and_then(|entry| entry.maintenance_warning.as_ref())
                {
                    warnings.push(format!("Could not maintain provider cache: {warning}"));
                }
                cached
            }
            Err(CacheError::Obsolete {
                path,
                found_schema,
                current_schema,
            }) => {
                warnings.push(format!(
                    "Ignored obsolete AcoustID cache entry {} (schema {found_schema}; current schema {current_schema})",
                    path.display()
                ));
                None
            }
            Err(CacheError::Damaged(path, error)) => {
                warnings.push(format!(
                    "Ignored damaged AcoustID cache entry {}: {error}",
                    path.display()
                ));
                None
            }
            Err(error) => {
                warnings.push(format!("Could not read AcoustID cache: {error}"));
                None
            }
        };

        if offline {
            return match cached {
                Some(entry) => AcoustIdLookup {
                    response: Some(entry.response),
                    origin: match entry.freshness {
                        MetadataFreshness::Fresh => AcoustIdLookupOrigin::FreshCache,
                        MetadataFreshness::Stale => AcoustIdLookupOrigin::OfflineStaleCache,
                    },
                    warnings,
                },
                None => AcoustIdLookup {
                    response: None,
                    origin: AcoustIdLookupOrigin::OfflineMiss,
                    warnings,
                },
            };
        }

        if !force_refresh
            && let Some(entry) = cached.as_ref()
            && entry.freshness == MetadataFreshness::Fresh
        {
            return AcoustIdLookup {
                response: Some(entry.response.clone()),
                origin: AcoustIdLookupOrigin::FreshCache,
                warnings,
            };
        }

        match self.provider.lookup(fingerprint, progress) {
            Ok(response) => {
                if let Err(error) = self.cache.store_acoustid(fingerprint, &response, now) {
                    warnings.push(format!("Could not update AcoustID cache: {error}"));
                }
                AcoustIdLookup {
                    response: Some(response),
                    origin: if force_refresh {
                        AcoustIdLookupOrigin::Refreshed
                    } else {
                        AcoustIdLookupOrigin::Live
                    },
                    warnings,
                }
            }
            Err(error) => match cached {
                Some(entry) => {
                    let (origin, cache_description) = match entry.freshness {
                        MetadataFreshness::Fresh => {
                            (AcoustIdLookupOrigin::FreshFallback, "cached identification")
                        }
                        MetadataFreshness::Stale => (
                            AcoustIdLookupOrigin::StaleFallback,
                            "stale cached identification",
                        ),
                    };
                    warnings.push(format!(
                        "AcoustID could not be refreshed ({error}); using {cache_description}"
                    ));
                    AcoustIdLookup {
                        response: Some(entry.response),
                        origin,
                        warnings,
                    }
                }
                None => {
                    warnings.push(format!(
                        "AcoustID is unavailable and no cached identification exists: {error}"
                    ));
                    AcoustIdLookup {
                        response: None,
                        origin: AcoustIdLookupOrigin::ProviderUnavailable,
                        warnings,
                    }
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcoustIdLookup {
    pub response: Option<AcoustIdResponse>,
    pub origin: AcoustIdLookupOrigin,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcoustIdLookupOrigin {
    Live,
    Refreshed,
    FreshCache,
    FreshFallback,
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
    use crate::provider::{AcoustIdResult, ProviderError};

    struct FakeProvider {
        calls: usize,
        result: Result<AcoustIdResponse, ProviderError>,
    }

    impl AcoustIdProvider for FakeProvider {
        fn lookup(
            &mut self,
            _fingerprint: &AudioFingerprint,
            _progress: &mut dyn ProviderProgress,
        ) -> Result<AcoustIdResponse, ProviderError> {
            self.calls += 1;
            std::mem::replace(
                &mut self.result,
                Err(ProviderError::Network("fake result consumed".into())),
            )
        }
    }

    #[test]
    fn fresh_no_match_bypasses_provider() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_acoustid(&fingerprint(), &AcoustIdResponse::default(), UNIX_EPOCH)
            .unwrap();
        let mut resolver = AcoustIdResolver::new(
            FakeProvider {
                calls: 0,
                result: Ok(match_response()),
            },
            cache,
        );

        let result = resolver.lookup(&fingerprint(), false, false, UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, AcoustIdLookupOrigin::FreshCache);
        assert!(result.response.unwrap().results.is_empty());
        assert_eq!(resolver.provider.calls, 0);
    }

    #[test]
    fn offline_cache_miss_does_not_call_provider_or_create_cache() {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("cache");
        let cache = ProviderCache::new(root.clone(), 1024 * 1024);
        let mut resolver = AcoustIdResolver::new(
            FakeProvider {
                calls: 0,
                result: Ok(match_response()),
            },
            cache,
        );

        let result = resolver.lookup(&fingerprint(), true, false, UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, AcoustIdLookupOrigin::OfflineMiss);
        assert_eq!(resolver.provider.calls, 0);
        assert!(!root.exists());
    }

    #[test]
    fn offline_mode_uses_stale_identification_without_provider() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_acoustid(&fingerprint(), &match_response(), UNIX_EPOCH)
            .unwrap();
        let mut resolver = AcoustIdResolver::new(
            FakeProvider {
                calls: 0,
                result: Err(ProviderError::Network("offline".into())),
            },
            cache,
        );

        let result = resolver.lookup(
            &fingerprint(),
            true,
            false,
            UNIX_EPOCH + Duration::from_secs(31 * 86_400),
            &mut (),
        );

        assert_eq!(result.origin, AcoustIdLookupOrigin::OfflineStaleCache);
        assert_eq!(resolver.provider.calls, 0);
    }

    #[test]
    fn failed_refresh_uses_stale_identification_visibly() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_acoustid(&fingerprint(), &match_response(), UNIX_EPOCH)
            .unwrap();
        let mut resolver = AcoustIdResolver::new(
            FakeProvider {
                calls: 0,
                result: Err(ProviderError::Network("temporary outage".into())),
            },
            cache,
        );

        let result = resolver.lookup(
            &fingerprint(),
            false,
            false,
            UNIX_EPOCH + Duration::from_secs(31 * 86_400),
            &mut (),
        );

        assert_eq!(result.origin, AcoustIdLookupOrigin::StaleFallback);
        assert!(result.response.is_some());
        assert!(result.warnings[0].contains("using stale cached identification"));
    }

    #[test]
    fn failed_forced_refresh_describes_fresh_identification_accurately() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_acoustid(&fingerprint(), &match_response(), UNIX_EPOCH)
            .unwrap();
        let mut resolver = AcoustIdResolver::new(
            FakeProvider {
                calls: 0,
                result: Err(ProviderError::Network("temporary outage".into())),
            },
            cache,
        );

        let result = resolver.lookup(&fingerprint(), false, true, UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, AcoustIdLookupOrigin::FreshFallback);
        assert!(result.response.is_some());
        assert!(result.warnings[0].contains("using cached identification"));
        assert!(!result.warnings[0].contains("stale"));
    }

    fn fingerprint() -> AudioFingerprint {
        AudioFingerprint {
            duration_seconds: 180,
            value: "fingerprint".into(),
        }
    }

    fn match_response() -> AcoustIdResponse {
        AcoustIdResponse {
            results: vec![AcoustIdResult {
                id: "result".into(),
                score: 0.95,
                recording_ids: vec!["recording".into()],
            }],
        }
    }
}
