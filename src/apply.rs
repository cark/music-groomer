mod cleanup;
mod copy;
mod file_copy;
pub(crate) mod publication;
mod space;
mod validation;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};

pub use cleanup::{
    AbandonedPartial, CleanupError, find as find_abandoned, remove as remove_abandoned,
};
use copy::{CopyError, copy_to_stage, groom};
use publication::{PublicationError, PublicationRoute, prepare_for_swap, publish};
use space::{SpaceWarning, required_space};
use validation::{ValidationError, validate};

use crate::plan::{ApplyReport, GroomingPlan};
use crate::replacement::{
    ReplacementContext, ReplacementSwap, ReplacementSwapReport, detect, swap_prepared,
};
use crate::source::{SourceInspection, SourceObjectKind, capture_snapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyStage {
    Preflight,
    Copying,
    Grooming,
    Validating,
    Publishing,
}

impl fmt::Display for ApplyStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Preflight => "Preflight",
            Self::Copying => "Copying source",
            Self::Grooming => "Grooming staged copy",
            Self::Validating => "Validating",
            Self::Publishing => "Publishing",
        })
    }
}

pub trait ApplyProgress {
    fn stage(&mut self, stage: ApplyStage) -> Result<(), String>;
}

impl ApplyProgress for () {
    fn stage(&mut self, _stage: ApplyStage) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ApplyEngine {
    temporary_root: PathBuf,
    force_destination_copy: bool,
}

impl Default for ApplyEngine {
    fn default() -> Self {
        Self {
            temporary_root: std::env::temp_dir(),
            force_destination_copy: false,
        }
    }
}

impl ApplyEngine {
    pub fn in_temporary_root(temporary_root: PathBuf) -> Self {
        Self {
            temporary_root,
            force_destination_copy: false,
        }
    }

    #[cfg(test)]
    fn force_destination_copy(mut self) -> Self {
        self.force_destination_copy = true;
        self
    }

    pub fn apply(
        &self,
        source: &SourceInspection,
        plan: &GroomingPlan,
        progress: &mut dyn ApplyProgress,
    ) -> Result<ApplyReport, ApplyFailure> {
        report_stage(progress, ApplyStage::Preflight)?;
        recheck_source(source)?;
        preflight_destination(source, plan)?;
        let required = required_space(content_bytes(source, plan));
        let mut warnings = Vec::new();
        check_space(&self.temporary_root, required, &mut warnings)?;
        check_space(&plan.destination_root, required, &mut warnings)?;

        let temporary = tempfile::Builder::new()
            .prefix("music-groomer-apply-")
            .tempdir_in(&self.temporary_root)
            .map_err(|error| {
                ApplyFailure::new(
                    ApplyStage::Preflight,
                    Some(self.temporary_root.clone()),
                    format!("cannot create temporary staging: {error}"),
                )
            })?;
        let stage = temporary.path().join("result");
        let operation: Result<_, ApplyFailure> = (|| {
            report_stage(progress, ApplyStage::Copying)?;
            copy_to_stage(source, plan, &stage).map_err(copy_failure(ApplyStage::Copying))?;
            report_stage(progress, ApplyStage::Grooming)?;
            groom(plan, &stage).map_err(copy_failure(ApplyStage::Grooming))?;
            report_stage(progress, ApplyStage::Validating)?;
            let validation = validate(source, plan, &stage).map_err(validation_failure)?;
            report_stage(progress, ApplyStage::Publishing)?;
            let publication = publish(
                &stage,
                &plan.destination_root,
                &plan.destination,
                self.force_destination_copy,
                |payload| {
                    validate(source, plan, payload)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                },
            )
            .map_err(publication_failure)?;
            Ok((validation, publication))
        })();

        match operation {
            Ok((validation, publication)) => {
                if let Some(warning) = publication.cleanup_warning {
                    warnings.push(format!(
                        "The result was published, but publication cleanup was incomplete: {warning}"
                    ));
                }
                if let Err(error) = temporary.close() {
                    warnings.push(format!(
                        "The result was published, but temporary cleanup failed: {error}"
                    ));
                }
                Ok(ApplyReport {
                    destination: plan.destination.clone(),
                    tracks_validated: validation.tracks,
                    artwork_validated: validation.artwork_files > 0,
                    source_unchanged: true,
                    warnings,
                    publication_copied: publication.route == PublicationRoute::DestinationCopy,
                })
            }
            Err(mut failure) => {
                let temporary_cleanup = temporary.close().err().map(|error| error.to_string());
                failure.cleanup = merge_cleanup(failure.cleanup, temporary_cleanup);
                Err(failure)
            }
        }
    }

    pub fn apply_replacement(
        &self,
        source: &SourceInspection,
        plan: &GroomingPlan,
        context: &ReplacementContext,
        retention: ReplacementRetention,
        progress: &mut dyn ApplyProgress,
    ) -> Result<ReplacementApplyReport, ApplyFailure> {
        report_stage(progress, ApplyStage::Preflight)?;
        recheck_source(source)?;
        let current = detect(source, plan).map_err(|error| {
            ApplyFailure::new(
                ApplyStage::Preflight,
                Some(context.active_path.clone()),
                error.to_string(),
            )
        })?;
        if current.as_ref() != Some(context) {
            return Err(ApplyFailure::new(
                ApplyStage::Preflight,
                Some(context.active_path.clone()),
                "replacement context changed after confirmation",
            ));
        }

        let required = required_space(content_bytes(source, plan));
        let mut warnings = Vec::new();
        check_space(&self.temporary_root, required, &mut warnings)?;
        check_space(&plan.destination_root, required, &mut warnings)?;
        let temporary = tempfile::Builder::new()
            .prefix("music-groomer-apply-")
            .tempdir_in(&self.temporary_root)
            .map_err(|error| {
                ApplyFailure::new(
                    ApplyStage::Preflight,
                    Some(self.temporary_root.clone()),
                    format!("cannot create temporary staging: {error}"),
                )
            })?;
        let stage = temporary.path().join("result");
        let operation: Result<_, ApplyFailure> = (|| {
            report_stage(progress, ApplyStage::Copying)?;
            copy_to_stage(source, plan, &stage).map_err(copy_failure(ApplyStage::Copying))?;
            report_stage(progress, ApplyStage::Grooming)?;
            groom(plan, &stage).map_err(copy_failure(ApplyStage::Grooming))?;
            report_stage(progress, ApplyStage::Validating)?;
            let validation = validate(source, plan, &stage).map_err(validation_failure)?;
            report_stage(progress, ApplyStage::Publishing)?;
            let prepared = prepare_for_swap(&stage, &plan.destination_root, |payload| {
                validate(source, plan, payload)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(publication_failure)?;
            let swap = ReplacementSwap {
                context: context.clone(),
                prepared_replacement: prepared.payload.clone(),
                display_label: retention.display_label,
                retained_at: retention.retained_at,
                protected_until: retention.protected_until,
            };
            match swap_prepared(&swap) {
                Ok(report) => {
                    if let Some(warning) = prepared.finish() {
                        warnings.push(format!(
                            "The replacement succeeded, but publication cleanup was incomplete: {warning}"
                        ));
                    }
                    Ok((validation, report))
                }
                Err(error) if error.rollback_incomplete() => {
                    let mut failure = ApplyFailure::new(
                        ApplyStage::Publishing,
                        Some(context.active_path.clone()),
                        error.to_string(),
                    );
                    failure.source_untouched = false;
                    failure.destination_published = true;
                    failure.cleanup = CleanupOutcome::Failed(format!(
                        "prepared replacement remains at {} for manual recovery",
                        prepared.payload.display()
                    ));
                    Err(failure)
                }
                Err(error) => {
                    let cleanup = prepared.discard().err().map(|error| error.to_string());
                    let mut failure = ApplyFailure::new(
                        ApplyStage::Publishing,
                        Some(context.active_path.clone()),
                        error.to_string(),
                    );
                    failure.cleanup =
                        cleanup.map_or(CleanupOutcome::Complete, CleanupOutcome::Failed);
                    Err(failure)
                }
            }
        })();

        match operation {
            Ok((validation, replacement)) => {
                if let Err(error) = temporary.close() {
                    warnings.push(format!(
                        "The replacement succeeded, but temporary cleanup failed: {error}"
                    ));
                }
                Ok(ReplacementApplyReport {
                    apply: ApplyReport {
                        destination: plan.destination.clone(),
                        tracks_validated: validation.tracks,
                        artwork_validated: validation.artwork_files > 0,
                        source_unchanged: false,
                        warnings,
                        publication_copied: true,
                    },
                    replacement,
                })
            }
            Err(mut failure) => {
                let temporary_cleanup = temporary.close().err().map(|error| error.to_string());
                failure.cleanup = merge_cleanup(failure.cleanup, temporary_cleanup);
                Err(failure)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementRetention {
    pub display_label: String,
    pub retained_at: u64,
    pub protected_until: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementApplyReport {
    pub apply: ApplyReport,
    pub replacement: ReplacementSwapReport,
}

fn report_stage(progress: &mut dyn ApplyProgress, stage: ApplyStage) -> Result<(), ApplyFailure> {
    progress
        .stage(stage)
        .map_err(|cause| ApplyFailure::new(stage, None, cause))
}

fn recheck_source(source: &SourceInspection) -> Result<(), ApplyFailure> {
    let current = capture_snapshot(&source.source, source.kind).map_err(|error| {
        let cause = error.to_string();
        ApplyFailure::source_changed(Some(error.path), cause)
    })?;
    if current == source.snapshot {
        return Ok(());
    }
    let changed = first_snapshot_difference(&source.snapshot, &current)
        .map(|path| source_path(source, &path));
    Err(ApplyFailure::source_changed(
        changed,
        "the selected source changed after inspection",
    ))
}

fn first_snapshot_difference(
    before: &[crate::source::SourceSnapshotEntry],
    after: &[crate::source::SourceSnapshotEntry],
) -> Option<PathBuf> {
    let mut before = before.iter().peekable();
    let mut after = after.iter().peekable();
    loop {
        match (before.peek(), after.peek()) {
            (Some(left), Some(right)) if left.relative_path == right.relative_path => {
                let left = before.next().expect("peeked");
                let right = after.next().expect("peeked");
                if left != right {
                    return Some(left.relative_path.clone());
                }
            }
            (Some(left), Some(right)) => {
                return Some(left.relative_path.clone().min(right.relative_path.clone()));
            }
            (Some(left), None) => return Some(left.relative_path.clone()),
            (None, Some(right)) => return Some(right.relative_path.clone()),
            (None, None) => return None,
        }
    }
}

fn source_path(source: &SourceInspection, relative: &Path) -> PathBuf {
    if source.kind == crate::domain::SourceKind::AlbumDirectory {
        source.source.join(relative)
    } else {
        source
            .source
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(relative)
    }
}

fn preflight_destination(
    source: &SourceInspection,
    plan: &GroomingPlan,
) -> Result<(), ApplyFailure> {
    if !plan.destination_root.is_dir() {
        return Err(ApplyFailure::new(
            ApplyStage::Preflight,
            Some(plan.destination_root.clone()),
            "the selected destination root is not an existing directory",
        ));
    }
    if source.kind == crate::domain::SourceKind::AlbumDirectory {
        let canonical_source = source.source.canonicalize().map_err(|error| {
            ApplyFailure::new(
                ApplyStage::Preflight,
                Some(source.source.clone()),
                format!("cannot resolve the selected source: {error}"),
            )
        })?;
        let canonical_root = plan.destination_root.canonicalize().map_err(|error| {
            ApplyFailure::new(
                ApplyStage::Preflight,
                Some(plan.destination_root.clone()),
                format!("cannot resolve the destination root: {error}"),
            )
        })?;
        let relative = plan
            .destination
            .strip_prefix(&plan.destination_root)
            .map_err(|_| {
                ApplyFailure::new(
                    ApplyStage::Preflight,
                    Some(plan.destination.clone()),
                    "the final destination is outside its selected root",
                )
            })?;
        let resolved_destination = canonical_root.join(relative);
        if resolved_destination.starts_with(&canonical_source) {
            return Err(ApplyFailure::new(
                ApplyStage::Preflight,
                Some(resolved_destination),
                "the result cannot be published inside the selected source album",
            ));
        }
    }
    match plan.destination.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(ApplyFailure::new(
            ApplyStage::Preflight,
            Some(plan.destination.clone()),
            "the release directory already exists; v0.1 cannot yet complete or rebuild an existing release piece by piece",
        )),
        Err(error) => Err(ApplyFailure::new(
            ApplyStage::Preflight,
            Some(plan.destination.clone()),
            format!("cannot check destination collision: {error}"),
        )),
    }
}

fn content_bytes(source: &SourceInspection, plan: &GroomingPlan) -> u64 {
    let source_bytes = source
        .snapshot
        .iter()
        .filter(|entry| entry.kind == SourceObjectKind::File)
        .map(|entry| entry.bytes)
        .fold(0_u64, u64::saturating_add);
    source_bytes.saturating_add(
        plan.archive_artwork_bytes
            .as_ref()
            .map_or(0, |bytes| bytes.len() as u64),
    )
}

fn check_space(path: &Path, required: u64, warnings: &mut Vec<String>) -> Result<(), ApplyFailure> {
    match space::check(path, required) {
        Ok(Some(SpaceWarning { path, cause })) => {
            warnings.push(format!(
                "Could not measure free space at {} ({cause}); writes will still fail cleanly if capacity runs out",
                path.display()
            ));
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(space) => Err(ApplyFailure::new(
            ApplyStage::Preflight,
            Some(space.path),
            format!(
                "insufficient free space: {} bytes required including margin, {} bytes available",
                space.required, space.available
            ),
        )),
    }
}

fn copy_failure(stage: ApplyStage) -> impl FnOnce(CopyError) -> ApplyFailure {
    move |error| ApplyFailure::new(stage, error.path().map(Path::to_owned), error.to_string())
}

fn validation_failure(error: ValidationError) -> ApplyFailure {
    ApplyFailure::new(ApplyStage::Validating, error.path, error.invariant)
}

fn publication_failure(error: PublicationError) -> ApplyFailure {
    let stage = if matches!(&error, PublicationError::Validation { .. }) {
        ApplyStage::Validating
    } else {
        ApplyStage::Publishing
    };
    let cleanup_failure = error.cleanup_failure();
    let mut failure = ApplyFailure::new(stage, Some(error.path().to_owned()), error.to_string());
    if let Some(cause) = cleanup_failure {
        failure.cleanup = CleanupOutcome::Failed(cause);
    }
    failure
}

fn merge_cleanup(existing: CleanupOutcome, temporary_failure: Option<String>) -> CleanupOutcome {
    match (existing, temporary_failure) {
        (CleanupOutcome::Failed(existing), Some(temporary)) => CleanupOutcome::Failed(format!(
            "{existing}; temporary staging cleanup also failed: {temporary}"
        )),
        (CleanupOutcome::Failed(existing), None) => CleanupOutcome::Failed(existing),
        (_, Some(temporary)) => {
            CleanupOutcome::Failed(format!("temporary staging cleanup failed: {temporary}"))
        }
        (_, None) => CleanupOutcome::Complete,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupOutcome {
    NotNeeded,
    Complete,
    Failed(String),
}

impl fmt::Display for CleanupOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotNeeded => formatter.write_str("no temporary data was created"),
            Self::Complete => formatter.write_str("temporary data was cleaned"),
            Self::Failed(cause) => write!(formatter, "temporary cleanup failed: {cause}"),
        }
    }
}

#[derive(Debug)]
pub struct ApplyFailure {
    pub stage: ApplyStage,
    pub path: Option<PathBuf>,
    pub cause: String,
    pub source_untouched: bool,
    pub destination_published: bool,
    pub cleanup: CleanupOutcome,
    pub requires_reinspection: bool,
}

impl ApplyFailure {
    fn new(stage: ApplyStage, path: Option<PathBuf>, cause: impl Into<String>) -> Self {
        Self {
            stage,
            path,
            cause: cause.into(),
            source_untouched: true,
            destination_published: false,
            cleanup: CleanupOutcome::NotNeeded,
            requires_reinspection: false,
        }
    }

    fn source_changed(path: Option<PathBuf>, cause: impl Into<String>) -> Self {
        let mut failure = Self::new(ApplyStage::Preflight, path, cause);
        failure.requires_reinspection = true;
        failure
    }
}

impl fmt::Display for ApplyFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed", self.stage)?;
        if let Some(path) = &self.path {
            write!(formatter, " at {}", path.display())?;
        }
        write!(formatter, ": {}; {}", self.cause, self.cleanup)
    }
}

impl std::error::Error for ApplyFailure {}
