use std::io;
use std::path::Path;

use crate::config::AppConfig;
use crate::destination::DestinationRoot;
use crate::guided_matching::GuidedMatchResult;
use crate::plan::GroomingPlan;
use crate::planning::build_plan;
use crate::source::SourceInspection;
use crate::terminal::{Action, ActionMenu, Interaction, MenuId, SemanticRole, UiLine};

pub fn initial_plan(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    config: &mut AppConfig,
    output: Option<&Path>,
) -> io::Result<Option<GroomingPlan>> {
    let proposed = output.or(config.destination.as_deref());
    if let Some(path) = proposed {
        match plan_at(source, matched, path) {
            Ok(plan) => return Ok(Some(plan)),
            Err(error) => {
                interaction.error(format!("The proposed destination cannot be used: {error}"))?
            }
        }
    } else {
        interaction.warning("No default destination is configured yet.")?;
    }
    choose(interaction, source, matched, config, None)
}

pub fn change(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    config: &mut AppConfig,
    current: GroomingPlan,
) -> io::Result<GroomingPlan> {
    Ok(choose(interaction, source, matched, config, Some(&current))?.unwrap_or(current))
}

fn choose(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    config: &mut AppConfig,
    current: Option<&GroomingPlan>,
) -> io::Result<Option<GroomingPlan>> {
    interaction.section_heading("Choose destination")?;
    interaction.prose("  The destination root must already exist.")?;
    loop {
        let answer = interaction.prompt(destination_prompt(current))?;
        if answer.is_empty() {
            return Ok(None);
        }
        let root = match DestinationRoot::existing(&answer) {
            Ok(root) => root,
            Err(error) => {
                interaction.error(format!("Not usable: {error}"))?;
                continue;
            }
        };
        if current.is_some_and(|plan| plan.destination_root == root.path()) {
            return Ok(None);
        }
        let proposed = match plan_at_root(source, matched, &root) {
            Ok(plan) => plan,
            Err(error) => {
                interaction.error(format!("Not usable: {error}"))?;
                continue;
            }
        };
        interaction.success("Destination is valid.")?;
        interaction.path_field(
            "Resulting release",
            proposed.destination.display().to_string(),
        )?;
        let menu = ActionMenu::for_id(MenuId::DestinationChoice);
        loop {
            let answer = interaction.prompt(menu.prompt("Choose: "))?;
            match menu.action(&answer) {
                Some(Action::UseOnce) => return Ok(Some(proposed)),
                Some(Action::SaveDefault) => {
                    config.destination = Some(proposed.destination_root.clone());
                    match config.save() {
                        Ok(()) => {
                            interaction.success("Default destination saved.")?;
                            return Ok(Some(proposed));
                        }
                        Err(error) => interaction
                            .error(format!("Could not save the default destination: {error}"))?,
                    }
                }
                Some(Action::Back) => break,
                _ => interaction.error("Please choose one of the displayed actions.")?,
            }
        }
    }
}

fn destination_prompt(current: Option<&GroomingPlan>) -> UiLine {
    if let Some(current) = current {
        UiLine::new()
            .with(SemanticRole::Prompt, "Destination root [")
            .with(
                SemanticRole::Path,
                current.destination_root.display().to_string(),
            )
            .with(SemanticRole::Prompt, "]: ")
    } else {
        UiLine::prompt("Destination root (Enter to go back): ")
    }
}

fn plan_at(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    path: &Path,
) -> Result<GroomingPlan, String> {
    let root = DestinationRoot::existing(&path.display().to_string())
        .map_err(|error| error.to_string())?;
    plan_at_root(source, matched, &root)
}

fn plan_at_root(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    root: &DestinationRoot,
) -> Result<GroomingPlan, String> {
    let plan = build_plan(source, matched, root.path()).map_err(|error| error.to_string())?;
    root.relocate(plan).map_err(|error| error.to_string())
}
