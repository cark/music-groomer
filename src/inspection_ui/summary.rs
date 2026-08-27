use std::collections::BTreeSet;
use std::io;

use crate::domain::SourceKind;
use crate::source::SourceInspection;
use crate::terminal::{Interaction, TextStyle};

use super::render::{artwork_summary, duration, show_label_value, show_notice, show_styled};

pub(super) fn show(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
    continues_to_metadata: bool,
) -> io::Result<()> {
    show_styled(
        interaction,
        TextStyle::Heading,
        "music-groomer — source inspection",
    )?;
    show_label_value(
        interaction,
        "Source",
        &inspection.source.display().to_string(),
        TextStyle::Path,
    )?;
    show_label_value(
        interaction,
        "Interpretation",
        interpretation(inspection),
        TextStyle::Value,
    )?;
    show_label_value(
        interaction,
        "Audio",
        &audio_summary(inspection),
        TextStyle::Value,
    )?;
    show_common(interaction, inspection, "Album", |audio| {
        audio.tags.album.as_deref()
    })?;
    show_common(interaction, inspection, "Album artist", |audio| {
        audio.tags.album_artist.as_deref()
    })?;
    show_common(interaction, inspection, "Date", |audio| {
        audio.tags.date.as_deref()
    })?;
    show_track_coverage(interaction, inspection)?;
    show_artwork(interaction, inspection)?;
    show_label_value(
        interaction,
        "Ancillary",
        &format!("{} file(s)", inspection.ancillary.len()),
        TextStyle::Value,
    )?;
    for notice in &inspection.notices {
        show_notice(interaction, notice)?;
    }
    show_completion(interaction, inspection)?;
    if continues_to_metadata {
        interaction.show(
            "The destination was not accessed; provider matching starts only after Continue.",
        )?;
    } else {
        interaction.show("No provider was contacted and no destination was accessed.")?;
    }
    interaction.show("")
}

fn interpretation(inspection: &SourceInspection) -> &'static str {
    match inspection.kind {
        SourceKind::AlbumDirectory if inspection.audio.len() == 1 => {
            "directory containing one release track (ancillary files included)"
        }
        SourceKind::AlbumDirectory => "one release directory (recursive)",
        SourceKind::LooseFile => "one loose audio file; sibling files excluded",
    }
}

fn audio_summary(inspection: &SourceInspection) -> String {
    if inspection.audio.is_empty() {
        return "none".to_owned();
    }
    let formats = inspection
        .audio
        .iter()
        .map(|audio| audio.format.to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} track(s), {}, {}",
        inspection.audio.len(),
        formats,
        duration(inspection.duration())
    )
}

fn show_common(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
    label: &str,
    value: impl Fn(&crate::source::InspectedAudio) -> Option<&str>,
) -> io::Result<()> {
    let values = inspection
        .audio
        .iter()
        .filter_map(value)
        .collect::<BTreeSet<_>>();
    let (text, style) = match values.len() {
        0 => ("(missing)".to_owned(), TextStyle::Warning),
        1 => (
            values.first().copied().unwrap_or_default().to_owned(),
            TextStyle::Value,
        ),
        _ => ("(tracks disagree)".to_owned(), TextStyle::Warning),
    };
    show_label_value(interaction, label, &text, style)
}

fn show_track_coverage(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    let numbered = inspection
        .audio
        .iter()
        .filter(|audio| audio.tags.track.is_some())
        .count();
    let discs = inspection
        .audio
        .iter()
        .filter_map(|audio| audio.tags.disc)
        .collect::<BTreeSet<_>>();
    let disc_label = if discs.is_empty() {
        "disc coverage unknown".to_owned()
    } else {
        format!("{} disc(s) represented", discs.len())
    };
    show_label_value(
        interaction,
        "Positions",
        &format!(
            "{numbered}/{} track(s) numbered; {disc_label}",
            inspection.audio.len()
        ),
        TextStyle::Value,
    )
}

fn show_artwork(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    match inspection
        .selected_artwork
        .and_then(|index| inspection.artwork.get(index))
    {
        Some(artwork) => show_label_value(
            interaction,
            "Artwork",
            &artwork_summary(artwork),
            TextStyle::Value,
        ),
        None if inspection.artwork.is_empty() => show_label_value(
            interaction,
            "Artwork",
            "no recognizable source front",
            TextStyle::Warning,
        ),
        None => show_label_value(
            interaction,
            "Artwork",
            "multiple equally preferred source fronts",
            TextStyle::Warning,
        ),
    }
}

fn show_completion(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    if inspection.is_blocked() {
        show_styled(
            interaction,
            TextStyle::Error,
            "Blocked: the source cannot be groomed safely until the errors above are resolved.",
        )?;
        interaction.show("The source remains untouched; inspection made no changes.")
    } else {
        show_styled(
            interaction,
            TextStyle::Success,
            "✓ Inspection complete; no files were changed.",
        )
    }
}
