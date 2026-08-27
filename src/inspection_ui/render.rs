use std::io;

use crate::source::{ArtworkCandidate, InspectionNotice, NoticeSeverity};
use crate::terminal::{Interaction, SemanticRole, UiLine};

pub(super) fn show_notice(
    interaction: &mut impl Interaction,
    notice: &InspectionNotice,
) -> io::Result<()> {
    let (label, role) = match notice.severity {
        NoticeSeverity::Warning => ("Warning", SemanticRole::Warning),
        NoticeSeverity::Blocker => ("Blocker", SemanticRole::Error),
    };
    let path = notice
        .path
        .as_ref()
        .map(|path| format!(" [{}]", path.display()))
        .unwrap_or_default();
    interaction.present(UiLine::new().with(role, format!("  {label}{path}: {}", notice.message)))
}

pub(super) fn show_label_value(
    interaction: &mut impl Interaction,
    label: &str,
    value: &str,
    role: SemanticRole,
) -> io::Result<()> {
    interaction.present(
        UiLine::new()
            .with(SemanticRole::Prose, "  ")
            .with(SemanticRole::FieldName, label)
            .with(SemanticRole::Prose, ": ")
            .with(role, value),
    )
}

pub(super) fn duration(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn optional(value: Option<impl ToString>) -> String {
    value.map_or_else(|| "?".to_owned(), |value| value.to_string())
}

pub(super) fn artwork_summary(artwork: &ArtworkCandidate) -> String {
    format!(
        "{} ({} {}x{})",
        artwork.relative_path.display(),
        artwork.format,
        artwork.dimensions.0,
        artwork.dimensions.1
    )
}
