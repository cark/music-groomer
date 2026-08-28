mod destination;
mod render;

#[cfg(test)]
mod tests;

use std::fmt;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::apply::{
    ApplyEngine, ApplyFailure, ApplyProgress, ApplyStage, ReplacementRetention, find_abandoned,
    remove_abandoned,
};
use crate::artwork_viewer::ArtworkViewer;
use crate::config::AppConfig;
use crate::guided_matching::{GuidedMatchResult, revise_artwork};
use crate::plan::GroomingPlan;
use crate::planning::build_plan;
use crate::recovery::retained_time_label;
use crate::replacement::{ReplacementContext, detect};
use crate::source::SourceInspection;
use crate::terminal::{Action, ActionMenu, Interaction, MenuId, SemanticRole, UiLine, byte_count};

#[derive(Debug)]
pub enum GuidedApplyError {
    Io(io::Error),
    Planning(String),
    SourceChanged(ApplyFailure),
    Replacement(String),
}

impl fmt::Display for GuidedApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "terminal interaction failed: {error}"),
            Self::Planning(error) => write!(formatter, "cannot build the grooming plan: {error}"),
            Self::SourceChanged(error) => write!(
                formatter,
                "the preview is no longer valid; inspect the source again ({error})"
            ),
            Self::Replacement(error) => write!(formatter, "replacement cannot proceed: {error}"),
        }
    }
}

impl std::error::Error for GuidedApplyError {}

impl From<io::Error> for GuidedApplyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn choose_initial_destination(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    config: &mut AppConfig,
    output: Option<&Path>,
) -> io::Result<Option<GroomingPlan>> {
    destination::initial_plan(interaction, source, matched, config, output)
}

pub fn run_with_plan<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    mut matched: GuidedMatchResult,
    mut config: AppConfig,
    mut plan: GroomingPlan,
    viewer: &mut V,
) -> Result<(), GuidedApplyError> {
    offer_abandoned_cleanup(interaction, &plan.destination_root)?;
    let menu = ActionMenu::for_id(MenuId::ExactPreview);

    loop {
        let replacement = detect(source, &plan)
            .map_err(|error| GuidedApplyError::Replacement(error.to_string()))?;
        let recovery_grace_days = replacement
            .as_ref()
            .map(|_| config.recovery_grace_days())
            .transpose()
            .map_err(|error| GuidedApplyError::Replacement(error.to_string()))?;
        render::summary(
            interaction,
            &plan,
            replacement.as_ref(),
            recovery_grace_days,
        )?;
        let answer = interaction.prompt(menu.prompt("Choose: "))?;
        match menu.action(&answer) {
            Some(Action::Apply) => {
                let confirmed = if let Some(replacement) = &replacement {
                    confirm_replacement(
                        interaction,
                        replacement,
                        recovery_grace_days.expect("replacement has a recovery grace period"),
                    )?
                } else {
                    confirm_apply(interaction, &plan)?
                };
                if !confirmed {
                    interaction.prose("Apply not confirmed; returning to the preview.")?;
                    continue;
                }
                match apply(interaction, source, &plan, replacement.as_ref(), &config)? {
                    ApplyOutcome::Applied => return Ok(()),
                    ApplyOutcome::Retry => {}
                    ApplyOutcome::Reinspect(failure) => {
                        return Err(GuidedApplyError::SourceChanged(failure));
                    }
                }
            }
            Some(Action::Review) => render::details(interaction, &plan)?,
            Some(Action::Artwork) => {
                revise_artwork(interaction, source, &mut matched, viewer)?;
                plan = build_plan(source, &matched, &plan.destination_root)
                    .map_err(|error| GuidedApplyError::Planning(error.to_string()))?;
            }
            Some(Action::Destination) => {
                let previous_root = plan.destination_root.clone();
                plan =
                    destination::change(interaction, source, &matched, &mut config, plan.clone())?;
                if plan.destination_root != previous_root {
                    offer_abandoned_cleanup(interaction, &plan.destination_root)?;
                }
            }
            Some(Action::Cancel) => {
                interaction.prose("Cancelled. The source and destination were not changed.")?;
                return Ok(());
            }
            None if answer.is_empty() => {}
            _ => interaction.error("Please choose one of the displayed actions.")?,
        }
    }
}

enum ApplyOutcome {
    Applied,
    Retry,
    Reinspect(ApplyFailure),
}

fn apply(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    plan: &GroomingPlan,
    replacement: Option<&ReplacementContext>,
    config: &AppConfig,
) -> Result<ApplyOutcome, GuidedApplyError> {
    interaction.section_heading("Applying confirmed plan")?;
    let result = {
        let mut progress = InteractionApplyProgress(interaction);
        if let Some(context) = replacement {
            let retained_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| GuidedApplyError::Replacement(error.to_string()))?
                .as_secs();
            let grace_seconds = config
                .recovery_grace_days()
                .map_err(|error| GuidedApplyError::Replacement(error.to_string()))?
                .checked_mul(24 * 60 * 60)
                .ok_or_else(|| {
                    GuidedApplyError::Replacement("recovery grace period is too large".into())
                })?;
            let protected_until = retained_at.checked_add(grace_seconds).ok_or_else(|| {
                GuidedApplyError::Replacement("recovery protection deadline overflows".into())
            })?;
            ApplyEngine::default()
                .apply_replacement(
                    source,
                    plan,
                    context,
                    ReplacementRetention {
                        display_label: replacement_display_label(context),
                        retained_at,
                        protected_until,
                    },
                    &mut progress,
                )
                .map(|report| (report.apply, Some(report.replacement)))
        } else {
            ApplyEngine::default()
                .apply(source, plan, &mut progress)
                .map(|report| (report, None))
        }
    };
    match result {
        Ok((report, replacement)) => {
            interaction.section_heading("Grooming complete")?;
            interaction.success("✓ The validated result is ready in the library.")?;
            interaction.path_field("Destination", report.destination.display().to_string())?;
            interaction.field("Tracks", format!("{} validated", report.tracks_validated))?;
            interaction.field(
                "Artwork",
                if report.artwork_validated {
                    "1 canonical sidecar validated"
                } else {
                    "no canonical sidecar selected"
                },
            )?;
            interaction.field("Validation", "passed")?;
            if let Some(replacement) = replacement {
                interaction.field("Selected release", "replaced after validation")?;
                let retained_time = retained_time_label(replacement.retained_at)
                    .unwrap_or_else(|_| "unknown retention time".into());
                let protected_until = retained_time_label(replacement.protected_until)
                    .unwrap_or_else(|_| "unknown protection deadline".into());
                let protection_days = replacement
                    .protected_until
                    .saturating_sub(replacement.retained_at)
                    / (24 * 60 * 60);
                interaction.success(format!(
                    "✓ Previous version safely stashed as {} · {retained_time}.",
                    replacement.display_label
                ))?;
                interaction.field(
                    "Protected from automatic cleanup",
                    format!("for at least {protection_days} days, until {protected_until}"),
                )?;
                interaction.prose(
                    "  After that date it becomes eligible for cleanup; it is not necessarily deleted then.",
                )?;
                interaction.prose(format!(
                    "  Run `music-groomer recovery` and choose “{} · {retained_time}” to restore it.",
                    replacement.display_label
                ))?;
            } else {
                interaction.field("Source", "untouched")?;
            }
            for warning in report.warnings {
                interaction.warning(format!("Warning: {warning}"))?;
            }
            Ok(ApplyOutcome::Applied)
        }
        Err(failure) => {
            show_failure(interaction, &failure)?;
            if failure.requires_reinspection {
                Ok(ApplyOutcome::Reinspect(failure))
            } else {
                interaction.prose(
                    "The unchanged preview is still available; change it, retry, or cancel.",
                )?;
                Ok(ApplyOutcome::Retry)
            }
        }
    }
}

fn show_failure(interaction: &mut impl Interaction, failure: &ApplyFailure) -> io::Result<()> {
    interaction.section_heading("Apply failed")?;
    interaction.error(format!("{} failed: {}", failure.stage, failure.cause))?;
    if let Some(path) = &failure.path {
        interaction.path_field("Affected path", path.display().to_string())?;
    }
    interaction.field(
        "Source",
        if failure.source_untouched {
            "untouched"
        } else {
            "state unknown"
        },
    )?;
    interaction.field(
        "Destination",
        if failure.destination_published {
            "published"
        } else {
            "no result was published"
        },
    )?;
    interaction.field("Cleanup", failure.cleanup.to_string())
}

fn confirm_apply(interaction: &mut impl Interaction, plan: &GroomingPlan) -> io::Result<bool> {
    loop {
        let answer = interaction
            .prompt(UiLine::confirmation_prompt(format!(
                "Apply this exact plan to {}? [Y/n]: ",
                plan.destination.display()
            )))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer Yes or No.")?,
        }
    }
}

fn confirm_replacement(
    interaction: &mut impl Interaction,
    replacement: &ReplacementContext,
    recovery_grace_days: u64,
) -> io::Result<bool> {
    interaction.section_heading("REPLACE EXISTING RELEASE")?;
    interaction.warning("Warning: the current library release will stop being active.")?;
    interaction.path_field(
        "Current release",
        replacement.active_path.display().to_string(),
    )?;
    interaction.path_field(
        "New active release",
        replacement.destination.display().to_string(),
    )?;
    interaction.prose("  The complete current version will be retained for recovery.")?;
    interaction.field(
        "Recovery protection",
        format!("at least {recovery_grace_days} days"),
    )?;
    loop {
        let answer = interaction
            .prompt(UiLine::confirmation_prompt(
                "Proceed with replacement? [y/N]: ",
            ))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer Yes or No.")?,
        }
    }
}

fn replacement_display_label(context: &ReplacementContext) -> String {
    let components = context
        .historical_path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match components.as_slice() {
        [] => "Retained release".into(),
        [release] => release.to_string(),
        components => format!(
            "{} — {}",
            components[components.len() - 2],
            components[components.len() - 1]
        ),
    }
}

fn offer_abandoned_cleanup(
    interaction: &mut impl Interaction,
    destination_root: &Path,
) -> io::Result<()> {
    let partials = match find_abandoned(destination_root) {
        Ok(partials) => partials,
        Err(error) => {
            interaction.warning(format!(
                "Could not inspect music-groomer's publication partials: {error}"
            ))?;
            return Ok(());
        }
    };
    for partial in partials {
        interaction.section_heading("Abandoned music-groomer publication")?;
        interaction.path_field("Partial", partial.path.display().to_string())?;
        interaction.field("Size", byte_count(partial.bytes))?;
        if confirm_remove(interaction)? {
            match remove_abandoned(&partial) {
                Ok(()) => interaction.success("Abandoned publication data removed.")?,
                Err(error) => interaction.warning(format!(
                    "Could not remove {}: {error}. This does not prevent a new Apply.",
                    partial.path.display()
                ))?,
            }
        } else {
            interaction.prose("Abandoned publication data left unchanged.")?;
        }
    }
    Ok(())
}

fn confirm_remove(interaction: &mut impl Interaction) -> io::Result<bool> {
    loop {
        let answer = interaction
            .prompt(UiLine::confirmation_prompt(
                "Remove this abandoned partial? [Y/n]: ",
            ))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer Yes or No.")?,
        }
    }
}

struct InteractionApplyProgress<'a, I>(&'a mut I);

impl<I: Interaction> ApplyProgress for InteractionApplyProgress<'_, I> {
    fn stage(&mut self, stage: ApplyStage) -> Result<(), String> {
        self.0
            .present(
                UiLine::new()
                    .with(SemanticRole::Prose, "  → ")
                    .with(SemanticRole::Selected, stage.to_string()),
            )
            .map_err(|error| error.to_string())
    }
}
