mod destination;
mod fixtures;
mod planning;
mod render;

#[cfg(test)]
mod tests;

use std::fmt;
use std::io;
use std::path::Path;

use crate::matching::{MatchDecision, MatchPolicy, RankedCandidate};
use crate::plan::{ApplyReport, GroomingPlan, MatchSelection};
pub use crate::terminal::{Interaction, StdioInteraction, TextStyle};

use destination::DestinationRoot;
use fixtures::{DemoData, demo_data};
use planning::{build_plan, coherent_standalone};
use render::{choose_artwork, show_details, show_inspection, show_styled, show_summary};

const PRETEND_LIBRARY_ROOT: &str = "/media/music";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoScenario {
    ConfidentAlbum,
    AmbiguousCollaboration,
    MatchedSingle,
    StandaloneTrack,
}

impl DemoScenario {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "confident" => Some(Self::ConfidentAlbum),
            "ambiguous" => Some(Self::AmbiguousCollaboration),
            "matched-single" => Some(Self::MatchedSingle),
            "standalone" => Some(Self::StandaloneTrack),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemoOutcome {
    Cancelled,
    Applied(ApplyReport),
}

#[derive(Debug)]
pub enum DemoError {
    Io(io::Error),
    InvalidDemoData(String),
}

impl fmt::Display for DemoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "terminal interaction failed: {error}"),
            Self::InvalidDemoData(message) => write!(formatter, "invalid demo data: {message}"),
        }
    }
}

impl std::error::Error for DemoError {}

impl From<io::Error> for DemoError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn run(
    interaction: &mut impl Interaction,
    scenario: Option<DemoScenario>,
    output_root: Option<&Path>,
) -> Result<DemoOutcome, DemoError> {
    show_styled(
        interaction,
        TextStyle::Heading,
        "music-groomer — guided preview demo",
    )?;
    interaction.show(
        "Simulation only: no music is read or written, no settings are saved, and no network requests are made.",
    )?;
    interaction.show("")?;

    let scenario = match scenario {
        Some(scenario) => scenario,
        None => match choose_scenario(interaction)? {
            Some(scenario) => scenario,
            None => {
                interaction.show("Cancelled. No files were written.")?;
                return Ok(DemoOutcome::Cancelled);
            }
        },
    };
    let data = demo_data(scenario);

    show_inspection(interaction, &data)?;
    let (selected, match_selection) = match select_metadata(interaction, &data)? {
        MetadataSelection::Matched {
            candidate,
            automatic,
        } => (
            Some(candidate),
            if automatic {
                MatchSelection::Automatic
            } else {
                MatchSelection::UserChosen
            },
        ),
        MetadataSelection::ExistingTags => (None, MatchSelection::ExistingTags),
        MetadataSelection::Cancelled => {
            interaction.show("Cancelled. No files were written.")?;
            return Ok(DemoOutcome::Cancelled);
        }
    };
    let mut plan = build_plan(
        &data,
        selected,
        match_selection,
        Path::new(PRETEND_LIBRARY_ROOT),
    )?;
    if let Some(output_root) = output_root {
        let root = DestinationRoot::existing(&output_root.display().to_string())
            .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
        plan = root
            .relocate(plan)
            .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
    }

    loop {
        show_summary(interaction, &plan)?;
        let action = interaction
            .ask(&action_prompt(interaction))?
            .to_ascii_lowercase();
        match action.as_str() {
            "a" | "apply" => {
                if confirm_apply(interaction, &plan)? {
                    let report = ApplyReport {
                        destination: plan.destination.clone(),
                        tracks_validated: plan.tracks.len(),
                        artwork_validated: plan.artwork.output_name.is_some(),
                        source_unchanged: true,
                        simulated: true,
                    };
                    interaction.show("")?;
                    show_styled(
                        interaction,
                        TextStyle::Success,
                        "Demo apply complete. No files were written.",
                    )?;
                    interaction.show(&format!(
                        "Would validate {} track(s) at {}.",
                        report.tracks_validated,
                        report.destination.display()
                    ))?;
                    interaction.show("The source would remain untouched.")?;
                    return Ok(DemoOutcome::Applied(report));
                }
                interaction.show("Apply not confirmed; returning to the preview.")?;
            }
            "r" | "review" => show_details(interaction, &plan)?,
            "w" | "artwork" => plan = choose_artwork(interaction, plan)?,
            "d" | "destination" => plan = choose_destination(interaction, plan)?,
            "c" | "cancel" | "q" | "quit" => {
                interaction.show("Cancelled. No files were written.")?;
                return Ok(DemoOutcome::Cancelled);
            }
            "" => {}
            _ => show_styled(
                interaction,
                TextStyle::Error,
                "Please choose Apply, Review, Artwork, Destination, or Cancel.",
            )?,
        }
    }
}

fn action_prompt(interaction: &impl Interaction) -> String {
    let apply = interaction.styled(TextStyle::Label, "[a] Apply");
    let review = interaction.styled(TextStyle::Label, "[r] Review");
    let artwork = interaction.styled(TextStyle::Label, "[w] Artwork");
    let destination = interaction.styled(TextStyle::Label, "[d] Destination");
    let cancel = interaction.styled(TextStyle::Label, "[c] Cancel");
    format!("Choose: {apply}  {review}  {artwork}  {destination}  {cancel}: ")
}

fn choose_scenario(interaction: &mut impl Interaction) -> Result<Option<DemoScenario>, DemoError> {
    show_styled(
        interaction,
        TextStyle::Heading,
        "Choose a pretend source to explore",
    )?;
    interaction.show("  1. Ordinary album with a clear match")?;
    interaction.show("  2. Collaboration album needing your choice")?;
    interaction.show("  3. Loose track matched to a single")?;
    interaction.show("  4. Loose track kept as a standalone track")?;
    loop {
        match interaction.ask("Source [1-4, or c to cancel]: ")?.as_str() {
            "1" => return Ok(Some(DemoScenario::ConfidentAlbum)),
            "2" => return Ok(Some(DemoScenario::AmbiguousCollaboration)),
            "3" => return Ok(Some(DemoScenario::MatchedSingle)),
            "4" => return Ok(Some(DemoScenario::StandaloneTrack)),
            "c" | "C" | "q" | "Q" => return Ok(None),
            _ => show_styled(
                interaction,
                TextStyle::Error,
                "Please enter 1, 2, 3, 4, or c.",
            )?,
        }
    }
}

fn select_metadata(
    interaction: &mut impl Interaction,
    data: &DemoData,
) -> Result<MetadataSelection, DemoError> {
    match MatchPolicy::default().decide(&data.inspection, data.candidates.clone()) {
        MatchDecision::Selected(selected) => {
            let label = interaction.styled(TextStyle::Value, &selected.candidate.human_label());
            interaction.show(&format!("Matched automatically: {label}"))?;
            for reason in selected.reasons.iter().take(3) {
                show_styled(
                    interaction,
                    TextStyle::Success,
                    &format!("  ✓ {}", reason.summary),
                )?;
            }
            interaction.show("")?;
            Ok(MetadataSelection::Matched {
                candidate: selected,
                automatic: true,
            })
        }
        MatchDecision::NeedsChoice(candidates) => {
            show_styled(
                interaction,
                TextStyle::Warning,
                "I found more than one plausible release. Which looks right?",
            )?;
            for (index, candidate) in candidates.iter().enumerate() {
                interaction.show(&format!(
                    "  {}. {}",
                    index + 1,
                    candidate.candidate.human_label()
                ))?;
            }
            loop {
                let answer = interaction.ask("Release number, or c to cancel: ")?;
                if matches!(answer.as_str(), "c" | "C" | "q" | "Q") {
                    return Ok(MetadataSelection::Cancelled);
                }
                if let Ok(index) = answer.parse::<usize>()
                    && let Some(candidate) = candidates.get(index.saturating_sub(1))
                {
                    let label =
                        interaction.styled(TextStyle::Value, &candidate.candidate.human_label());
                    interaction.show(&format!("Using: {label}"))?;
                    interaction.show("")?;
                    return Ok(MetadataSelection::Matched {
                        candidate: Box::new(candidate.clone()),
                        automatic: false,
                    });
                }
                show_styled(
                    interaction,
                    TextStyle::Error,
                    "Please enter one of the displayed release numbers, or c.",
                )?;
            }
        }
        MatchDecision::NoUsableMatch(_) => {
            if coherent_standalone(data) {
                interaction.show("No matching single was found.")?;
                interaction.show(
                    "The existing artist and title are coherent, so this can remain a standalone track.",
                )?;
                interaction.show("")?;
                Ok(MetadataSelection::ExistingTags)
            } else {
                Err(DemoError::InvalidDemoData(
                    "no usable match or coherent standalone metadata".into(),
                ))
            }
        }
    }
}

enum MetadataSelection {
    Matched {
        candidate: Box<RankedCandidate>,
        automatic: bool,
    },
    ExistingTags,
    Cancelled,
}

fn choose_destination(
    interaction: &mut impl Interaction,
    plan: GroomingPlan,
) -> Result<GroomingPlan, DemoError> {
    interaction.show("")?;
    show_styled(interaction, TextStyle::Heading, "Change destination")?;
    interaction.show(&format!(
        "  Current root: {}",
        interaction.styled(
            TextStyle::Path,
            &plan.destination_root.display().to_string()
        )
    ))?;
    interaction.show("The new root must already exist. Enter b to go back.")?;

    loop {
        let answer = interaction.ask("Destination root: ")?;
        if matches!(answer.to_ascii_lowercase().as_str(), "b" | "back") {
            return Ok(plan);
        }
        let root = match DestinationRoot::existing(&answer) {
            Ok(root) => root,
            Err(error) => {
                show_styled(
                    interaction,
                    TextStyle::Error,
                    &format!("Not usable: {error}"),
                )?;
                continue;
            }
        };
        let proposed = match root.relocate(plan.clone()) {
            Ok(proposed) => proposed,
            Err(error) => {
                show_styled(
                    interaction,
                    TextStyle::Error,
                    &format!("Not usable: {error}"),
                )?;
                continue;
            }
        };
        show_styled(interaction, TextStyle::Success, "Destination is valid.")?;
        interaction.show(&format!(
            "  Resulting album: {}",
            interaction.styled(TextStyle::Path, &proposed.destination.display().to_string())
        ))?;

        loop {
            match interaction
                .ask("Choose: [o] Use once  [s] Use and save as default  [b] Go back: ")?
                .to_ascii_lowercase()
                .as_str()
            {
                "o" | "once" => return Ok(proposed),
                "s" | "save" => {
                    show_styled(
                        interaction,
                        TextStyle::Success,
                        &format!(
                            "Demo only: would save {} as the default destination.",
                            root.path().display()
                        ),
                    )?;
                    return Ok(proposed);
                }
                "b" | "back" => return Ok(plan),
                _ => show_styled(
                    interaction,
                    TextStyle::Error,
                    "Please choose Use once, Save as default, or Go back.",
                )?,
            }
        }
    }
}

fn confirm_apply(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
) -> Result<bool, DemoError> {
    loop {
        let answer = interaction
            .ask(&format!(
                "Apply this exact plan to {}? [Y/n]: ",
                plan.destination.display()
            ))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => show_styled(interaction, TextStyle::Error, "Please answer yes or no.")?,
        }
    }
}
