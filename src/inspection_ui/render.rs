use std::io;

use crate::source::{ArtworkCandidate, InspectionNotice, NoticeSeverity};
use crate::terminal::{Interaction, TextStyle};

pub(super) fn show_notice(
    interaction: &mut impl Interaction,
    notice: &InspectionNotice,
) -> io::Result<()> {
    let (label, style) = match notice.severity {
        NoticeSeverity::Warning => ("Warning", TextStyle::Warning),
        NoticeSeverity::Blocker => ("Blocker", TextStyle::Error),
    };
    let path = notice
        .path
        .as_ref()
        .map(|path| format!(" [{}]", path.display()))
        .unwrap_or_default();
    show_styled(
        interaction,
        style,
        &format!("  {label}{path}: {}", notice.message),
    )
}

pub(super) fn show_label_value(
    interaction: &mut impl Interaction,
    label: &str,
    value: &str,
    style: TextStyle,
) -> io::Result<()> {
    let label = interaction.styled(TextStyle::Label, label);
    let value = interaction.styled(style, value);
    interaction.show(&format!("  {label}: {value}"))
}

pub(super) fn show_styled(
    interaction: &mut impl Interaction,
    style: TextStyle,
    text: &str,
) -> io::Result<()> {
    let text = interaction.styled(style, text);
    interaction.show(&text)
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
