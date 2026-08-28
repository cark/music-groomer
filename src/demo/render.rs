use crate::domain::SourceKind;
use crate::plan::{GroomingPlan, MatchSelection, MetadataBasis};

use super::fixtures::DemoData;
use super::{DemoError, Interaction, SemanticRole, UiLine};

pub(super) fn show_inspection(
    interaction: &mut impl Interaction,
    data: &DemoData,
) -> Result<(), DemoError> {
    interaction.heading("Inspection")?;
    interaction.path_field("Source", &data.inspection.source_label)?;
    let kind = match data.inspection.kind {
        SourceKind::AlbumDirectory => "album directory",
        SourceKind::LooseFile => "one loose audio track",
    };
    interaction.prose(format!(
        "  Found: {} {} file(s); treating this as {kind}.",
        data.inspection.tracks.len(),
        data.extensions
            .first()
            .map_or("audio", |extension| extension.as_str())
            .to_ascii_uppercase()
    ))?;
    interaction.success("  ✓ Source remains read-only.")?;
    interaction.blank()?;
    Ok(())
}

pub(super) fn show_summary(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
) -> Result<(), DemoError> {
    interaction.heading("Preview")?;
    match &plan.metadata {
        MetadataBasis::MusicBrainz(release) => {
            interaction.field("Metadata", release.human_label())?;
            match plan.match_selection {
                MatchSelection::Automatic => {
                    if let Some(reason) = plan.match_reasons.first() {
                        interaction.present(
                            UiLine::new()
                                .with(SemanticRole::Prose, "  ")
                                .with(SemanticRole::FieldName, "Why automatic")
                                .with(SemanticRole::Prose, ": ")
                                .with(SemanticRole::Success, reason),
                        )?;
                    }
                }
                MatchSelection::UserChosen => {
                    interaction.field("Decision", "selected by you from the plausible matches")?
                }
                MatchSelection::ExistingTags => {}
            }
        }
        MetadataBasis::ExistingTags => {
            interaction.warning("  Metadata: existing tags (not verified against MusicBrainz)")?
        }
    }
    interaction.path_field("Destination", plan.destination.display().to_string())?;
    interaction.field("Artwork", plan.artwork.description())?;
    interaction.field(
        "Changes",
        format!(
            "{} tag value(s), {} filename(s)",
            plan.tag_change_count(),
            plan.filename_change_count()
        ),
    )?;
    interaction.field(
        "Preserved",
        format!(
            "embedded artwork in {} track(s)",
            plan.preserved_embedded_artwork
        ),
    )?;
    for warning in &plan.warnings {
        interaction.warning(format!("  Warning: {}", warning.summary))?;
    }
    interaction.blank()?;
    Ok(())
}

pub(super) fn show_details(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
) -> Result<(), DemoError> {
    interaction.blank()?;
    interaction.heading("All planned changes")?;
    for track in &plan.tracks {
        interaction.present(UiLine::new().with(
            SemanticRole::Value,
            format!("  {}", track.source_relative.display()),
        ))?;
        interaction.present(UiLine::new().with(
            SemanticRole::Path,
            format!("    → {}", track.destination.display()),
        ))?;
        if track.tag_changes.is_empty() {
            interaction.prose("    tags unchanged")?;
        } else {
            for change in &track.tag_changes {
                interaction.present(
                    UiLine::new()
                        .with(SemanticRole::Prose, "    ")
                        .with(SemanticRole::FieldName, change.field.to_string())
                        .with(SemanticRole::Prose, ": ")
                        .with(
                            SemanticRole::Warning,
                            change.before.as_deref().unwrap_or("(missing)"),
                        )
                        .with(SemanticRole::Prose, " → ")
                        .with(SemanticRole::Success, &change.after),
                )?;
            }
        }
        interaction.success("    ✓ embedded artwork preserved unchanged")?;
    }
    if let Some(output_name) = &plan.artwork.output_name {
        interaction.present(
            UiLine::new()
                .with(SemanticRole::FieldName, "  Sidecar artwork")
                .with(SemanticRole::Prose, ": ")
                .with(SemanticRole::Value, &plan.artwork.label)
                .with(SemanticRole::Prose, " → ")
                .with(SemanticRole::Path, output_name),
        )?;
    }
    for warning in &plan.warnings {
        interaction.warning(format!(
            "  Warning: {} — {}",
            warning.summary, warning.detail
        ))?;
    }
    interaction.blank()?;
    Ok(())
}

pub(super) fn choose_artwork(
    interaction: &mut impl Interaction,
    plan: GroomingPlan,
) -> Result<GroomingPlan, DemoError> {
    let mut choices = vec![plan.artwork.clone()];
    choices.extend(plan.artwork_alternatives.clone());
    if choices.len() == 1 {
        interaction.prose("No alternative artwork is available for this preview.")?;
        return Ok(plan);
    }

    interaction.blank()?;
    interaction.heading("Artwork choices")?;
    for (index, artwork) in choices.iter().enumerate() {
        let selected = artwork == &plan.artwork;
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "  ")
                .with(SemanticRole::MenuKey, format!("{}.", index + 1))
                .with(SemanticRole::Prose, " ")
                .with(
                    if selected {
                        SemanticRole::Selected
                    } else {
                        SemanticRole::Alternative
                    },
                    format!(
                        "{}{}",
                        artwork.description(),
                        if selected { " (selected)" } else { "" }
                    ),
                ),
        )?;
    }
    interaction.prose("  v. View a choice (simulated in this demo)")?;
    loop {
        let answer = interaction.prompt(UiLine::prompt(
            "Artwork number, v to view, or b to go back: ",
        ))?;
        match answer.to_ascii_lowercase().as_str() {
            "b" | "back" => return Ok(plan),
            "v" | "view" => {
                let number = interaction.prompt(UiLine::prompt("View which artwork number? "))?;
                if let Ok(index) = number.parse::<usize>()
                    && let Some(artwork) = choices.get(index.saturating_sub(1))
                {
                    interaction.prose(format!(
                        "Would open {} in the normal image viewer.",
                        artwork.description()
                    ))?;
                    continue;
                }
                interaction.error("Please choose one of the displayed artwork numbers.")?;
            }
            _ => {
                if let Ok(index) = answer.parse::<usize>()
                    && let Some(artwork) = choices.get(index.saturating_sub(1))
                {
                    interaction.success(format!("Selected: {}", artwork.description()))?;
                    interaction.blank()?;
                    return Ok(plan.with_artwork(artwork.clone()));
                }
                interaction.error("Please choose an artwork number, v, or b.")?;
            }
        }
    }
}
