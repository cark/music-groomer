use std::time::{Duration, Instant};

use super::http::ProviderHttp;
use super::{ProviderError, ProviderProgress};
use crate::source::ArtworkFormat;

const RETRY_DEADLINE: Duration = Duration::from_secs(60);

pub trait ArtworkProvider {
    fn front(
        &mut self,
        release_group_id: &str,
        progress: &mut dyn ProviderProgress,
    ) -> Result<Option<ProviderArtwork>, ProviderError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderArtwork {
    pub bytes: Vec<u8>,
    pub format: ArtworkFormat,
    pub dimensions: (u32, u32),
}

pub struct CoverArtArchive {
    http: ProviderHttp,
}

impl CoverArtArchive {
    pub fn new() -> Self {
        Self {
            http: ProviderHttp::new(),
        }
    }
}

impl Default for CoverArtArchive {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtworkProvider for CoverArtArchive {
    fn front(
        &mut self,
        release_group_id: &str,
        progress: &mut dyn ProviderProgress,
    ) -> Result<Option<ProviderArtwork>, ProviderError> {
        let url =
            format!("https://coverartarchive.org/release-group/{release_group_id}/front-1200");
        let bytes = match self.http.get_bytes(
            &url,
            "Cover Art Archive front",
            Duration::ZERO,
            Instant::now() + RETRY_DEADLINE,
            progress,
        ) {
            Ok(bytes) => bytes,
            Err(ProviderError::HttpStatus { status: 404, .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        decode(bytes).map(Some)
    }
}

pub(super) fn decode(bytes: Vec<u8>) -> Result<ProviderArtwork, ProviderError> {
    let guessed = image::guess_format(&bytes)
        .map_err(|error| ProviderError::InvalidResponse(format!("artwork format: {error}")))?;
    let format = match guessed {
        image::ImageFormat::Jpeg => ArtworkFormat::Jpeg,
        image::ImageFormat::Png => ArtworkFormat::Png,
        image::ImageFormat::WebP => ArtworkFormat::WebP,
        image::ImageFormat::Gif => ArtworkFormat::Gif,
        other => {
            return Err(ProviderError::InvalidResponse(format!(
                "unsupported artwork format {other:?}"
            )));
        }
    };
    let image = image::load_from_memory_with_format(&bytes, guessed)
        .map_err(|error| ProviderError::InvalidResponse(format!("artwork image: {error}")))?;
    Ok(ProviderArtwork {
        bytes,
        format,
        dimensions: (image.width(), image.height()),
    })
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use std::io::Cursor;

    use super::*;

    #[test]
    fn validates_downloaded_artwork_and_keeps_native_format() {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::new(4, 5))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();

        let artwork = decode(bytes).unwrap();

        assert_eq!(artwork.format, ArtworkFormat::Png);
        assert_eq!(artwork.dimensions, (4, 5));
    }
}
