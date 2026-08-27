use super::{ArtworkProvider, CacheError, ProviderArtwork, ProviderCache, ProviderProgress};

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
        progress: &mut dyn ProviderProgress,
    ) -> ArtworkLookup {
        let mut warnings = Vec::new();
        let cached = match self.cache.artwork(release_group_id) {
            Ok(cached) => cached.map(|entry| entry.artwork),
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
                artwork: cached,
                origin: ArtworkLookupOrigin::OfflineCache,
                warnings,
            };
        }
        if !force_refresh && cached.is_some() {
            return ArtworkLookup {
                artwork: cached,
                origin: ArtworkLookupOrigin::Cache,
                warnings,
            };
        }

        match self.provider.front(release_group_id, progress) {
            Ok(artwork) => {
                if let Some(artwork) = artwork.as_ref()
                    && let Err(error) = self.cache.store_artwork(release_group_id, artwork)
                {
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
            Err(error) if cached.is_some() => {
                warnings.push(format!(
                    "Cover Art Archive could not be refreshed ({error}); using cached artwork"
                ));
                ArtworkLookup {
                    artwork: cached,
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

        let result = resolver.lookup("group", false, false, &mut ());

        assert_eq!(result.origin, ArtworkLookupOrigin::Cache);
        assert!(result.artwork.is_some());
        assert_eq!(resolver.provider.calls, 0);
    }

    fn artwork() -> ProviderArtwork {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::new(2, 3))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        super::super::cover_art_archive::decode(bytes).unwrap()
    }
}
