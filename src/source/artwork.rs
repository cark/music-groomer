use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use image::{ImageFormat, ImageReader};

use super::ArtworkFormat;

pub(super) enum ArtworkProbe {
    Supported {
        format: ArtworkFormat,
        dimensions: (u32, u32),
    },
    Unsupported(String),
    NotImage,
}

pub(super) fn probe(path: &Path) -> io::Result<ArtworkProbe> {
    let span = tracing::trace_span!("probe_artwork", path = %path.display());
    let _entered = span.enter();
    let mut file = File::open(path)?;
    let mut header = [0_u8; 32];
    let read = file.read(&mut header)?;
    let guessed = image::guess_format(&header[..read]);

    match guessed {
        Ok(format) => match supported_format(format) {
            Some(format) => {
                let dimensions = ImageReader::open(path)?
                    .with_guessed_format()?
                    .into_dimensions();
                match dimensions {
                    Ok(dimensions) => Ok(ArtworkProbe::Supported { format, dimensions }),
                    Err(error) => Ok(ArtworkProbe::Unsupported(format!(
                        "{format} image failed validation: {error}"
                    ))),
                }
            }
            None => Ok(ArtworkProbe::Unsupported(format_label(format))),
        },
        Err(_) if probable_image_extension(path) => Ok(ArtworkProbe::Unsupported(
            "unrecognized or damaged image".to_owned(),
        )),
        Err(_) => Ok(ArtworkProbe::NotImage),
    }
}

pub(super) fn name_priority(path: &Path) -> Option<u8> {
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    match stem.as_str() {
        "cover" => Some(0),
        "folder" => Some(1),
        "front" => Some(2),
        "albumart" => Some(3),
        _ => None,
    }
}

pub(super) fn probable_image_extension(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "avif"
            | "heic"
            | "heif"
            | "jxl"
    )
}

fn supported_format(format: ImageFormat) -> Option<ArtworkFormat> {
    match format {
        ImageFormat::Jpeg => Some(ArtworkFormat::Jpeg),
        ImageFormat::Png => Some(ArtworkFormat::Png),
        ImageFormat::WebP => Some(ArtworkFormat::WebP),
        ImageFormat::Gif => Some(ArtworkFormat::Gif),
        _ => None,
    }
}

fn format_label(format: ImageFormat) -> String {
    format!("{format:?}")
}
