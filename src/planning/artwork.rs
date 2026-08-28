use std::path::PathBuf;

use crate::guided_matching::{ArtworkSelection, GuidedMatchResult};
use crate::matching_ui::MetadataSelection;
use crate::plan::{AncillaryPlan, ArtworkChoice, ArtworkOrigin};
use crate::source::{SourceInspection, SourceObjectKind};

pub(super) fn artwork_plan(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
) -> (ArtworkChoice, Option<Vec<u8>>) {
    match &matched.artwork {
        ArtworkSelection::Source(index) => {
            let candidate = source.artwork.get(*index);
            let choice = candidate.map_or_else(no_artwork, |candidate| ArtworkChoice {
                origin: ArtworkOrigin::SourceSidecar {
                    source_name: candidate.relative_path.clone(),
                },
                label: format!("Source {}", candidate.relative_path.display()),
                dimensions: Some(candidate.dimensions),
                output_name: Some(format!("cover.{}", candidate.format.canonical_extension())),
            });
            (choice, None)
        }
        ArtworkSelection::CoverArtArchive(artwork) => {
            let release_group_id = match &matched.metadata {
                MetadataSelection::Provider(selected) => selected
                    .candidate
                    .release_group_id
                    .clone()
                    .unwrap_or_default(),
                MetadataSelection::ExistingTags | MetadataSelection::Cancelled => String::new(),
            };
            (
                ArtworkChoice {
                    origin: ArtworkOrigin::CoverArtArchive { release_group_id },
                    label: "Cover Art Archive front".into(),
                    dimensions: Some(artwork.dimensions),
                    output_name: Some(format!("cover.{}", artwork.format.canonical_extension())),
                },
                Some(artwork.bytes.clone()),
            )
        }
        ArtworkSelection::None => (no_artwork(), None),
    }
}

pub(super) fn ancillary_plan(
    source: &SourceInspection,
    artwork: &ArtworkChoice,
) -> Vec<AncillaryPlan> {
    source
        .ancillary
        .iter()
        .filter_map(|file| {
            let destination_relative = match &artwork.origin {
                ArtworkOrigin::SourceSidecar { source_name }
                    if file.relative_path == source_name.as_path() =>
                {
                    return None;
                }
                ArtworkOrigin::SourceSidecar { .. } | ArtworkOrigin::CoverArtArchive { .. }
                    if source
                        .artwork
                        .iter()
                        .any(|candidate| candidate.relative_path == file.relative_path) =>
                {
                    PathBuf::from("original-artwork").join(&file.relative_path)
                }
                ArtworkOrigin::SourceSidecar { .. }
                | ArtworkOrigin::CoverArtArchive { .. }
                | ArtworkOrigin::None => file.relative_path.clone(),
            };
            Some(AncillaryPlan {
                source_relative: file.relative_path.clone(),
                destination_relative,
            })
        })
        .collect()
}

pub(super) fn ancillary_directories(
    source: &SourceInspection,
    ancillary: &[AncillaryPlan],
) -> Vec<PathBuf> {
    let audio_paths = source
        .audio
        .iter()
        .map(|audio| &audio.relative_path)
        .collect::<Vec<_>>();
    source
        .snapshot
        .iter()
        .filter(|entry| {
            entry.kind == SourceObjectKind::Directory && !entry.relative_path.as_os_str().is_empty()
        })
        .filter(|entry| {
            ancillary
                .iter()
                .any(|file| file.source_relative.starts_with(&entry.relative_path))
                || !audio_paths
                    .iter()
                    .any(|audio| audio.starts_with(&entry.relative_path))
        })
        .map(|entry| entry.relative_path.clone())
        .collect()
}

fn no_artwork() -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::None,
        label: "No sidecar artwork".into(),
        dimensions: None,
        output_name: None,
    }
}
