use std::io;

use crate::source::SourceInspection;
use crate::terminal::{Interaction, TextStyle};

use super::render::{artwork_summary, duration, optional, show_notice, show_styled};

pub(super) fn show(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    interaction.show("")?;
    show_styled(interaction, TextStyle::Heading, "Files and tags")?;
    for audio in &inspection.audio {
        show_audio(interaction, audio)?;
    }
    show_artwork(interaction, inspection)?;
    show_ancillary(interaction, inspection)?;
    if !inspection.notices.is_empty() {
        show_styled(interaction, TextStyle::Label, "  Warnings and blockers")?;
        for notice in &inspection.notices {
            show_notice(interaction, notice)?;
        }
    }
    interaction.show("")
}

fn show_audio(
    interaction: &mut impl Interaction,
    audio: &crate::source::InspectedAudio,
) -> io::Result<()> {
    show_styled(
        interaction,
        TextStyle::Label,
        &format!("  {}", audio.relative_path.display()),
    )?;
    interaction.show(&format!(
        "    {} · {} · {} Hz · {} channel(s)",
        audio.format,
        duration(audio.properties.duration),
        optional(audio.properties.sample_rate),
        optional(audio.properties.channels)
    ))?;
    show_tag(interaction, "Title", audio.tags.title.as_deref())?;
    show_tag(interaction, "Artist", audio.tags.artist.as_deref())?;
    show_values(interaction, "Artists", &audio.tags.artists)?;
    show_tag(interaction, "Album", audio.tags.album.as_deref())?;
    show_tag(
        interaction,
        "Album artist",
        audio.tags.album_artist.as_deref(),
    )?;
    show_values(interaction, "Album artists", &audio.tags.album_artists)?;
    show_tag(interaction, "Date", audio.tags.date.as_deref())?;
    interaction.show(&format!(
        "    Position: disc {} of {}, track {} of {}",
        optional(audio.tags.disc),
        optional(audio.tags.disc_total),
        optional(audio.tags.track),
        optional(audio.tags.track_total)
    ))?;
    interaction.show(&format!(
        "    Embedded artwork: {} picture(s), preserved",
        audio.tags.embedded_pictures
    ))?;
    show_tag(
        interaction,
        "MusicBrainz recording ID",
        audio.tags.recording_id.as_deref(),
    )?;
    show_tag(
        interaction,
        "MusicBrainz release-group ID",
        audio.tags.release_group_id.as_deref(),
    )?;
    show_extension_change(interaction, audio)
}

fn show_extension_change(
    interaction: &mut impl Interaction,
    audio: &crate::source::InspectedAudio,
) -> io::Result<()> {
    let source_extension = audio
        .relative_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if source_extension.eq_ignore_ascii_case(audio.format.canonical_extension()) {
        return Ok(());
    }
    let destination = audio
        .relative_path
        .with_extension(audio.format.canonical_extension());
    show_styled(
        interaction,
        TextStyle::Warning,
        &format!(
            "    Eventual filename correction: {} → {}",
            audio.relative_path.display(),
            destination.display()
        ),
    )
}

fn show_artwork(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    if inspection.artwork.is_empty() {
        return Ok(());
    }
    show_styled(interaction, TextStyle::Label, "  Source artwork candidates")?;
    for (index, artwork) in inspection.artwork.iter().enumerate() {
        let selected = if inspection.selected_artwork == Some(index) {
            " (selected)"
        } else {
            ""
        };
        interaction.show(&format!("    {}{selected}", artwork_summary(artwork)))?;
        let canonical = format!("cover.{}", artwork.format.canonical_extension());
        if artwork.relative_path.to_string_lossy() != canonical {
            interaction.show(&format!(
                "      Eventual sidecar: {} → {canonical}",
                artwork.relative_path.display()
            ))?;
        }
    }
    Ok(())
}

fn show_ancillary(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    if inspection.ancillary.is_empty() {
        return Ok(());
    }
    show_styled(interaction, TextStyle::Label, "  Ancillary files")?;
    for file in &inspection.ancillary {
        interaction.show(&format!(
            "    {} ({} bytes, preserved)",
            file.relative_path.display(),
            file.bytes
        ))?;
    }
    Ok(())
}

fn show_tag(
    interaction: &mut impl Interaction,
    label: &str,
    value: Option<&str>,
) -> io::Result<()> {
    interaction.show(&format!("    {label}: {}", value.unwrap_or("(missing)")))
}

fn show_values(
    interaction: &mut impl Interaction,
    label: &str,
    values: &[String],
) -> io::Result<()> {
    interaction.show(&format!(
        "    {label}: {}",
        if values.is_empty() {
            "(missing)".to_owned()
        } else {
            values.join("; ")
        }
    ))
}
