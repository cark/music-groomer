use crate::domain::SourceKind;
use crate::plan::{GroomingPlan, MatchSelection, MetadataBasis};

use super::fixtures::DemoData;
use super::{DemoError, Interaction, TextStyle};

pub(super) fn show_inspection(
    interaction: &mut impl Interaction,
    data: &DemoData,
) -> Result<(), DemoError> {
    show_styled(interaction, TextStyle::Heading, "Inspection")?;
    show_label_value(
        interaction,
        "Source",
        &data.inspection.source_label,
        TextStyle::Path,
    )?;
    let kind = match data.inspection.kind {
        SourceKind::AlbumDirectory => "album directory",
        SourceKind::LooseFile => "one loose audio track",
    };
    interaction.show(&format!(
        "  Found: {} {} file(s); treating this as {kind}.",
        data.inspection.tracks.len(),
        data.extensions
            .first()
            .map_or("audio", |extension| extension.as_str())
            .to_ascii_uppercase()
    ))?;
    show_styled(
        interaction,
        TextStyle::Success,
        "  ✓ Source remains read-only.",
    )?;
    interaction.show("")?;
    Ok(())
}

pub(super) fn show_summary(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
) -> Result<(), DemoError> {
    show_styled(interaction, TextStyle::Heading, "Preview")?;
    match &plan.metadata {
        MetadataBasis::MusicBrainz(release) => {
            show_label_value(
                interaction,
                "Metadata",
                &release.human_label(),
                TextStyle::Value,
            )?;
            match plan.match_selection {
                MatchSelection::Automatic => {
                    if let Some(reason) = plan.match_reasons.first() {
                        show_label_value(interaction, "Why automatic", reason, TextStyle::Success)?;
                    }
                }
                MatchSelection::UserChosen => {
                    interaction.show("  Decision: selected by you from the plausible matches")?
                }
                MatchSelection::ExistingTags => {}
            }
        }
        MetadataBasis::ExistingTags => show_label_value(
            interaction,
            "Metadata",
            "existing tags (not verified against MusicBrainz)",
            TextStyle::Warning,
        )?,
    }
    show_label_value(
        interaction,
        "Destination",
        &plan.destination.display().to_string(),
        TextStyle::Path,
    )?;
    show_label_value(
        interaction,
        "Artwork",
        &plan.artwork.description(),
        TextStyle::Value,
    )?;
    interaction.show(&format!(
        "  Changes: {} tag value(s), {} filename(s)",
        plan.tag_change_count(),
        plan.filename_change_count()
    ))?;
    interaction.show(&format!(
        "  Preserved: embedded artwork in {} track(s)",
        plan.preserved_embedded_artwork
    ))?;
    for warning in &plan.warnings {
        show_styled(
            interaction,
            TextStyle::Warning,
            &format!("  Warning: {}", warning.summary),
        )?;
    }
    interaction.show("")?;
    Ok(())
}

pub(super) fn show_details(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
) -> Result<(), DemoError> {
    interaction.show("")?;
    show_styled(interaction, TextStyle::Heading, "All planned changes")?;
    for track in &plan.tracks {
        show_styled(
            interaction,
            TextStyle::Label,
            &format!("  {}", track.source_name),
        )?;
        let path = interaction.styled(TextStyle::Path, &track.destination.display().to_string());
        interaction.show(&format!("    → {path}"))?;
        if track.tag_changes.is_empty() {
            interaction.show("    tags unchanged")?;
        } else {
            for change in &track.tag_changes {
                let field = interaction.styled(TextStyle::Label, &change.field.to_string());
                let before = interaction.styled(
                    TextStyle::Warning,
                    change.before.as_deref().unwrap_or("(missing)"),
                );
                let after = interaction.styled(TextStyle::Success, &change.after);
                interaction.show(&format!("    {field}: {before} → {after}"))?;
            }
        }
        show_styled(
            interaction,
            TextStyle::Success,
            "    ✓ embedded artwork preserved unchanged",
        )?;
    }
    if let Some(output_name) = &plan.artwork.output_name {
        let artwork = interaction.styled(TextStyle::Value, &plan.artwork.label);
        let output = interaction.styled(TextStyle::Path, output_name);
        interaction.show(&format!("  Sidecar artwork: {artwork} → {output}"))?;
    }
    for warning in &plan.warnings {
        show_styled(
            interaction,
            TextStyle::Warning,
            &format!("  Warning: {} — {}", warning.summary, warning.detail),
        )?;
    }
    interaction.show("")?;
    Ok(())
}

pub(super) fn choose_artwork(
    interaction: &mut impl Interaction,
    plan: GroomingPlan,
) -> Result<GroomingPlan, DemoError> {
    let mut choices = vec![plan.artwork.clone()];
    choices.extend(plan.artwork_alternatives.clone());
    if choices.len() == 1 {
        interaction.show("No alternative artwork is available for this preview.")?;
        return Ok(plan);
    }

    interaction.show("")?;
    show_styled(interaction, TextStyle::Heading, "Artwork choices")?;
    for (index, artwork) in choices.iter().enumerate() {
        let selected = if artwork == &plan.artwork {
            interaction.styled(TextStyle::Success, " (selected)")
        } else {
            String::new()
        };
        interaction.show(&format!(
            "  {}. {}{}",
            index + 1,
            artwork.description(),
            selected
        ))?;
    }
    interaction.show("  v. View a choice (simulated in this demo)")?;
    loop {
        let answer = interaction.ask("Artwork number, v to view, or b to go back: ")?;
        match answer.to_ascii_lowercase().as_str() {
            "b" | "back" => return Ok(plan),
            "v" | "view" => {
                let number = interaction.ask("View which artwork number? ")?;
                if let Ok(index) = number.parse::<usize>()
                    && let Some(artwork) = choices.get(index.saturating_sub(1))
                {
                    interaction.show(&format!(
                        "Would open {} in the normal image viewer.",
                        artwork.description()
                    ))?;
                    continue;
                }
                show_styled(
                    interaction,
                    TextStyle::Error,
                    "Please choose one of the displayed artwork numbers.",
                )?;
            }
            _ => {
                if let Ok(index) = answer.parse::<usize>()
                    && let Some(artwork) = choices.get(index.saturating_sub(1))
                {
                    show_styled(
                        interaction,
                        TextStyle::Success,
                        &format!("Selected: {}", artwork.description()),
                    )?;
                    interaction.show("")?;
                    return Ok(plan.with_artwork(artwork.clone()));
                }
                show_styled(
                    interaction,
                    TextStyle::Error,
                    "Please choose an artwork number, v, or b.",
                )?;
            }
        }
    }
}

pub(super) fn show_styled(
    interaction: &mut impl Interaction,
    style: TextStyle,
    text: &str,
) -> Result<(), DemoError> {
    let text = interaction.styled(style, text);
    interaction.show(&text)?;
    Ok(())
}

fn show_label_value(
    interaction: &mut impl Interaction,
    label: &str,
    value: &str,
    value_style: TextStyle,
) -> Result<(), DemoError> {
    let label = interaction.styled(TextStyle::Label, label);
    let value = interaction.styled(value_style, value);
    interaction.show(&format!("  {label}: {value}"))?;
    Ok(())
}
