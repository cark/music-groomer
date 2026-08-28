use std::io;

use crate::plan::{GroomingPlan, MatchSelection, MetadataBasis};
use crate::replacement::ReplacementContext;
use crate::terminal::{Interaction, SemanticRole, UiLine};

pub fn summary(
    interaction: &mut impl Interaction,
    plan: &GroomingPlan,
    replacement: Option<&ReplacementContext>,
    recovery_grace_days: Option<u64>,
) -> io::Result<()> {
    interaction.section_heading("Exact grooming preview")?;
    if let Some(replacement) = replacement {
        interaction.warning("  REPLACEMENT: this selected library release will be replaced")?;
        interaction.path_field(
            "Current release",
            replacement.active_path.display().to_string(),
        )?;
        interaction.path_field(
            "Replacement path",
            replacement.destination.display().to_string(),
        )?;
        interaction.prose("  The complete current release will be retained for recovery.")?;
        interaction.field(
            "Recovery protection",
            format!(
                "at least {} days",
                recovery_grace_days.expect("replacement preview has a recovery grace period")
            ),
        )?;
    }
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
                    interaction.field("Decision", "selected by you")?;
                }
                MatchSelection::ExistingTags => {}
            }
        }
        MetadataBasis::ExistingTags => {
            interaction.warning("  Metadata: existing tags (unverified)")?;
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
            "{} ancillary file(s), embedded artwork in {} track(s)",
            plan.ancillary.len(),
            plan.preserved_embedded_artwork
        ),
    )?;
    if plan.warnings.is_empty() {
        interaction.field("Warnings", "none")?;
    } else {
        interaction.field("Warnings", plan.warnings.len().to_string())?;
    }
    if replacement.is_some() {
        interaction.prose("  Nothing changes until replacement Apply is explicitly confirmed.")?;
    } else {
        interaction
            .prose("  The source remains untouched. Nothing changes until Apply is confirmed.")?;
    }
    interaction.blank()
}

pub fn details(interaction: &mut impl Interaction, plan: &GroomingPlan) -> io::Result<()> {
    interaction.section_heading("All planned changes")?;
    for track in &plan.tracks {
        interaction.present(UiLine::new().with(
            SemanticRole::Path,
            format!("  {}", track.source_relative.display()),
        ))?;
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "    ")
                .with(SemanticRole::Prose, "→ ")
                .with(SemanticRole::Path, track.destination.display().to_string()),
        )?;
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
                            SemanticRole::Alternative,
                            change.before.as_deref().unwrap_or("(missing)"),
                        )
                        .with(SemanticRole::Prose, " → ")
                        .with(SemanticRole::Selected, &change.after),
                )?;
            }
        }
    }
    if !plan.ancillary.is_empty() || !plan.ancillary_directories.is_empty() {
        interaction.heading("  Preserved ancillary data")?;
        for file in &plan.ancillary {
            interaction.present(
                UiLine::new()
                    .with(
                        SemanticRole::Path,
                        format!("    {}", file.source_relative.display()),
                    )
                    .with(SemanticRole::Prose, " → ")
                    .with(
                        SemanticRole::Path,
                        file.destination_relative.display().to_string(),
                    ),
            )?;
        }
        for directory in &plan.ancillary_directories {
            interaction.path_field("Directory", directory.display().to_string())?;
        }
    }
    if let Some(output_name) = &plan.artwork.output_name {
        interaction.field(
            "Canonical artwork",
            format!("{} → {output_name}", plan.artwork.description()),
        )?;
    } else {
        interaction.field("Canonical artwork", "none")?;
    }
    if !plan.warnings.is_empty() {
        interaction.heading("  Warnings")?;
        for warning in &plan.warnings {
            interaction.warning(format!("    Warning: {}", warning.summary))?;
        }
    }
    interaction.blank()
}
