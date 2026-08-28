mod artwork;
mod changes;
mod release;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use artwork::{ancillary_directories, ancillary_plan, artwork_plan};
use release::{existing_plan, provider_plan};

use crate::guided_matching::GuidedMatchResult;
use crate::layout::LayoutError;
use crate::matching_ui::MetadataSelection;
use crate::plan::{GroomingPlan, PlanWarning};
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
    let warnings = plan_warnings(source, matched, &tracks, &ancillary, &destination);
    let plan = GroomingPlan {
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
        artwork_alternatives: Vec::new(),
        warnings,
        preserved_embedded_artwork: source
            .audio
            .iter()
            .filter(|audio| audio.tags.embedded_pictures > 0)
            .count(),
        archive_artwork_bytes,
    };
    ensure_unique_outputs(&plan)?;
    Ok(plan)
}

fn plan_warnings(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    tracks: &[crate::plan::TrackPlan],
    ancillary: &[crate::plan::AncillaryPlan],
    destination: &Path,
) -> Vec<PlanWarning> {
    let mut seen = BTreeSet::new();
    let mut warnings = matched
        .warnings
        .iter()
        .filter(|warning| seen.insert((*warning).clone()))
        .map(|warning| PlanWarning {
            summary: warning.clone(),
            detail: warning.clone(),
        })
        .collect::<Vec<_>>();
    let audio_paths_change = tracks.iter().any(|track| {
        track.destination.strip_prefix(destination).ok() != Some(track.source_relative.as_path())
    });
    if !audio_paths_change {
        return warnings;
    }
    for file in ancillary.iter().filter(|file| {
        ["cue", "m3u", "m3u8"].iter().any(|extension| {
            file.source_relative
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
    }) {
        let already_warned = source.notices.iter().any(|notice| {
            notice.kind == crate::source::NoticeKind::StaleReference
                && notice.path.as_ref() == Some(&file.source_relative)
        });
        if !already_warned {
            let path = file.source_relative.display();
            let summary =
                format!("{path}: planned audio renames may leave preserved references stale");
            warnings.push(PlanWarning {
                summary: summary.clone(),
                detail: summary,
            });
        }
    }
    warnings
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
