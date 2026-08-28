use std::io;
use std::path::Path;

use crate::config::AppConfig;
use crate::destination::DestinationRoot;
use crate::guided_matching::GuidedMatchResult;
use crate::plan::GroomingPlan;
use crate::planning::build_plan;
use crate::source::SourceInspection;
use crate::terminal::{Interaction, UiLine};

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
    if let Some(current) = current {
        interaction.path_field(
            "Current root",
            current.destination_root.display().to_string(),
        )?;
    }
    interaction.prose("  The destination root must already exist. Enter c to cancel.")?;
    loop {
        let answer = interaction.prompt(UiLine::prompt("Destination root: "))?;
        if matches!(
            answer.to_ascii_lowercase().as_str(),
            "c" | "cancel" | "b" | "back"
        ) {
            return Ok(None);
        }
        let proposed = match plan_at(source, matched, Path::new(&answer)) {
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
        loop {
            let answer = interaction
                .prompt(UiLine::menu_prompt(
                    "Choose: [o] Use once  [s] Use and save as default  [b] Go back: ",
                ))?
                .to_ascii_lowercase();
            match answer.as_str() {
                "o" | "once" => return Ok(Some(proposed)),
                "s" | "save" => {
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
                "b" | "back" => break,
                _ => interaction.error("Please choose Use once, Save, or Go back.")?,
            }
        }
    }
}

fn plan_at(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    path: &Path,
) -> Result<GroomingPlan, String> {
    let root = DestinationRoot::existing(&path.display().to_string())
        .map_err(|error| error.to_string())?;
    let plan = build_plan(source, matched, root.path()).map_err(|error| error.to_string())?;
    root.relocate(plan).map_err(|error| error.to_string())
}
