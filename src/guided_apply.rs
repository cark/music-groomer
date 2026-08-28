mod destination;
mod render;

#[cfg(test)]
mod tests;

use std::fmt;
use std::io;
use std::path::Path;

use crate::apply::{
    ApplyEngine, ApplyFailure, ApplyProgress, ApplyStage, find_abandoned, remove_abandoned,
};
use crate::artwork_viewer::ArtworkViewer;
use crate::config::AppConfig;
use crate::guided_matching::{GuidedMatchResult, revise_artwork};
use crate::plan::GroomingPlan;
use crate::planning::build_plan;
use crate::source::SourceInspection;
use crate::terminal::{Action, ActionMenu, Interaction, MenuId, SemanticRole, UiLine, byte_count};

#[derive(Debug)]
pub enum GuidedApplyError {
    Io(io::Error),
    Planning(String),
    SourceChanged(ApplyFailure),
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
        render::summary(interaction, &plan)?;
        let answer = interaction.prompt(menu.prompt("Choose: "))?;
        match menu.action(&answer) {
            Some(Action::Apply) => {
                if !confirm_apply(interaction, &plan)? {
                    interaction.prose("Apply not confirmed; returning to the preview.")?;
                    continue;
                }
                match apply(interaction, source, &plan)? {
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
) -> Result<ApplyOutcome, GuidedApplyError> {
    interaction.section_heading("Applying confirmed plan")?;
    let result = {
        let mut progress = InteractionApplyProgress(interaction);
        ApplyEngine::default().apply(source, plan, &mut progress)
    };
    match result {
        Ok(report) => {
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
            interaction.field("Source", "untouched")?;
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
