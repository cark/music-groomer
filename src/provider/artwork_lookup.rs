use std::time::SystemTime;

use super::{
    ArtworkCacheEntry, ArtworkProvider, CacheError, MetadataFreshness, ProviderArtwork,
    ProviderCache, ProviderProgress,
};

pub struct ArtworkResolver<P> {
    provider: P,
    cache: ProviderCache,
}

impl<P: ArtworkProvider> ArtworkResolver<P> {
    pub fn new(provider: P, cache: ProviderCache) -> Self {
        Self { provider, cache }
    }

    pub fn lookup(
        &mut self,
        release_group_id: &str,
        offline: bool,
        force_refresh: bool,
        now: SystemTime,
        progress: &mut dyn ProviderProgress,
    ) -> ArtworkLookup {
        let mut warnings = Vec::new();
        let cached = match self.cache.artwork(release_group_id, now) {
            Ok(cached) => cached,
            Err(CacheError::Obsolete {
                path,
                found_schema,
                current_schema,
            }) => {
                warnings.push(format!(
                    "Ignored obsolete artwork cache entry {} (schema {found_schema}; current schema {current_schema})",
                    path.display()
                ));
                None
            }
            Err(CacheError::Damaged(path, error)) => {
                warnings.push(format!(
                    "Ignored damaged artwork cache entry {}: {error}",
                    path.display()
                ));
                None
            }
            Err(error) => {
                warnings.push(format!("Could not read artwork cache: {error}"));
                None
            }
        };

        if offline {
            return ArtworkLookup {
                artwork: cached_image(&cached),
                origin: ArtworkLookupOrigin::OfflineCache,
                warnings,
            };
        }
        if !force_refresh {
            match &cached {
                Some(ArtworkCacheEntry::Image(artwork)) => {
                    return ArtworkLookup {
                        artwork: Some(artwork.clone()),
                        origin: ArtworkLookupOrigin::Cache,
                        warnings,
                    };
                }
                Some(ArtworkCacheEntry::ConfirmedAbsent {
                    freshness: MetadataFreshness::Fresh,
                }) => {
                    return ArtworkLookup {
                        artwork: None,
                        origin: ArtworkLookupOrigin::ConfirmedAbsentCache,
                        warnings,
                    };
                }
                Some(ArtworkCacheEntry::ConfirmedAbsent {
                    freshness: MetadataFreshness::Stale,
                })
                | None => {}
            }
        }

        match self.provider.front(release_group_id, progress) {
            Ok(artwork) => {
                let stored = match artwork.as_ref() {
                    Some(artwork) => self.cache.store_artwork(release_group_id, artwork),
                    None => self.cache.store_artwork_absence(release_group_id, now),
                };
                if let Err(error) = stored {
                    warnings.push(format!("Could not update artwork cache: {error}"));
                }
                ArtworkLookup {
                    artwork,
                    origin: if force_refresh {
                        ArtworkLookupOrigin::Refreshed
                    } else {
                        ArtworkLookupOrigin::Live
                    },
                    warnings,
                }
            }
            Err(error) if cached_image(&cached).is_some() => {
                warnings.push(format!(
                    "Cover Art Archive could not be refreshed ({error}); using cached artwork"
                ));
                ArtworkLookup {
                    artwork: cached_image(&cached),
                    origin: ArtworkLookupOrigin::CacheFallback,
                    warnings,
                }
            }
            Err(error) => {
                warnings.push(format!("Cover Art Archive is unavailable: {error}"));
                ArtworkLookup {
                    artwork: None,
                    origin: ArtworkLookupOrigin::ProviderUnavailable,
                    warnings,
                }
            }
        }
    }
}

fn cached_image(cached: &Option<ArtworkCacheEntry>) -> Option<ProviderArtwork> {
    match cached {
        Some(ArtworkCacheEntry::Image(artwork)) => Some(artwork.clone()),
        Some(ArtworkCacheEntry::ConfirmedAbsent { .. }) | None => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkLookup {
    pub artwork: Option<ProviderArtwork>,
    pub origin: ArtworkLookupOrigin,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtworkLookupOrigin {
    Live,
    Refreshed,
    Cache,
    ConfirmedAbsentCache,
    CacheFallback,
    OfflineCache,
    ProviderUnavailable,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbaImage};
    use tempfile::TempDir;

    use super::*;
    use crate::provider::ProviderError;

    struct FakeArtworkProvider {
        calls: usize,
        result: Result<Option<ProviderArtwork>, ProviderError>,
    }

    impl ArtworkProvider for FakeArtworkProvider {
        fn front(
            &mut self,
            _release_group_id: &str,
            _progress: &mut dyn ProviderProgress,
        ) -> Result<Option<ProviderArtwork>, ProviderError> {
            self.calls += 1;
            std::mem::replace(
                &mut self.result,
                Err(ProviderError::Network("fake result consumed".into())),
            )
        }
    }

    #[test]
    fn cached_artwork_bypasses_archive() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache.store_artwork("group", &artwork()).unwrap();
        let provider = FakeArtworkProvider {
            calls: 0,
            result: Ok(None),
        };
        let mut resolver = ArtworkResolver::new(provider, cache);

        let result = resolver.lookup("group", false, false, std::time::UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, ArtworkLookupOrigin::Cache);
        assert!(result.artwork.is_some());
        assert_eq!(resolver.provider.calls, 0);
    }

    #[test]
    fn fresh_confirmed_absence_bypasses_archive() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        cache
            .store_artwork_absence("group", std::time::UNIX_EPOCH)
            .unwrap();
        let provider = FakeArtworkProvider {
            calls: 0,
            result: Ok(Some(artwork())),
        };
        let mut resolver = ArtworkResolver::new(provider, cache);

        let result = resolver.lookup("group", false, false, std::time::UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, ArtworkLookupOrigin::ConfirmedAbsentCache);
        assert!(result.artwork.is_none());
        assert_eq!(resolver.provider.calls, 0);
    }

    #[test]
    fn transient_failure_is_not_cached_as_confirmed_absence() {
        let temporary = TempDir::new().unwrap();
        let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
        let provider = FakeArtworkProvider {
            calls: 0,
            result: Err(ProviderError::Network("temporary".into())),
        };
        let mut resolver = ArtworkResolver::new(provider, cache.clone());

        let result = resolver.lookup("group", false, false, std::time::UNIX_EPOCH, &mut ());

        assert_eq!(result.origin, ArtworkLookupOrigin::ProviderUnavailable);
        assert!(
            cache
                .artwork("group", std::time::UNIX_EPOCH)
                .unwrap()
                .is_none()
        );
    }

    fn artwork() -> ProviderArtwork {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::new(2, 3))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        super::super::cover_art_archive::decode(bytes).unwrap()
    }
}
