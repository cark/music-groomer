mod artwork;
mod changes;
mod release;
mod warnings;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use artwork::{ancillary_directories, ancillary_plan, artwork_plan};
use release::{existing_plan, provider_plan};

use crate::guided_matching::GuidedMatchResult;
use crate::layout::LayoutError;
use crate::matching_ui::MetadataSelection;
use crate::plan::GroomingPlan;
use crate::provider::source_inspection;
use crate::source::SourceInspection;

#[derive(Debug)]
pub enum PlanningError {
    Cancelled,
    Missing(&'static str),
    InvalidMapping(usize),
    Layout(LayoutError),
    Collision(PathBuf),
}

impl fmt::Display for PlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("metadata selection was cancelled"),
            Self::Missing(field) => write!(formatter, "selected metadata has no {field}"),
            Self::InvalidMapping(index) => {
                write!(
                    formatter,
                    "selected metadata has an invalid track mapping at {index}"
                )
            }
            Self::Layout(error) => write!(formatter, "cannot plan destination layout: {error}"),
            Self::Collision(path) => write!(
                formatter,
                "more than one planned output would use {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PlanningError {}

impl From<LayoutError> for PlanningError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

pub fn build_plan(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    destination_root: &Path,
) -> Result<GroomingPlan, PlanningError> {
    let (inspection, _) = source_inspection(source);
    let (metadata, reasons, layout, tracks) = match &matched.metadata {
        MetadataSelection::Provider(selected) => {
            provider_plan(source, &inspection.tracks, selected, destination_root)?
        }
        MetadataSelection::ExistingTags => {
            existing_plan(source, &inspection.tracks, destination_root)?
        }
        MetadataSelection::Cancelled => return Err(PlanningError::Cancelled),
    };
    let (artwork, archive_artwork_bytes) = artwork_plan(source, matched);
    let ancillary = ancillary_plan(source, &artwork);
    let ancillary_directories = ancillary_directories(source, &ancillary);
    let destination = destination_root.join(&layout.directory);
    let mut plan = GroomingPlan {
        source_label: source.source.display().to_string(),
        metadata,
        match_selection: matched.match_selection,
        match_reasons: reasons,
        destination_root: destination_root.to_owned(),
        destination,
        tracks,
        ancillary,
        ancillary_directories,
        artwork,
        warnings: Vec::new(),
        preserved_embedded_artwork: source
            .audio
            .iter()
            .filter(|audio| audio.tags.embedded_pictures > 0)
            .count(),
        archive_artwork_bytes,
    };
    plan.warnings = warnings::for_plan(source, matched, &plan);
    ensure_unique_outputs(&plan)?;
    Ok(plan)
}

fn ensure_unique_outputs(plan: &GroomingPlan) -> Result<(), PlanningError> {
    let mut files = BTreeMap::new();
    for path in plan
        .tracks
        .iter()
        .map(|track| {
            track
                .destination
                .strip_prefix(&plan.destination)
                .map(Path::to_owned)
                .map_err(|_| PlanningError::Collision(track.destination.clone()))
        })
        .chain(
            plan.ancillary
                .iter()
                .map(|file| Ok(file.destination_relative.clone())),
        )
        .chain(
            plan.artwork
                .output_name
                .iter()
                .map(|name| Ok(PathBuf::from(name))),
        )
    {
        let path = path?;
        if files.insert(portable_path(&path), path.clone()).is_some() {
            return Err(PlanningError::Collision(path));
        }
    }
    let directories = plan
        .ancillary_directories
        .iter()
        .map(|path| portable_path(path))
        .collect::<Vec<_>>();
    for (portable_file, file) in &files {
        if directories
            .iter()
            .any(|directory| directory == portable_file || directory.starts_with(portable_file))
            || files
                .keys()
                .any(|other| other != portable_file && other.starts_with(portable_file))
        {
            return Err(PlanningError::Collision(file.clone()));
        }
    }
    Ok(())
}

fn portable_path(path: &Path) -> PathBuf {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

pub fn source_root(source: &SourceInspection) -> PathBuf {
    if source.source.is_dir() {
        source.source.clone()
    } else {
        source
            .source
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_owned()
    }
}
