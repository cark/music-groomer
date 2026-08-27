use std::io;

use crate::source::SourceInspection;
use crate::terminal::{Interaction, SemanticRole, UiLine};

use super::render::{artwork_summary, duration, optional, show_notice};

pub(super) fn show(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    interaction.blank()?;
    interaction.heading("Files and tags")?;
    for audio in &inspection.audio {
        show_audio(interaction, audio)?;
    }
    show_artwork(interaction, inspection)?;
    show_ancillary(interaction, inspection)?;
    if !inspection.notices.is_empty() {
        interaction.heading("  Warnings and blockers")?;
        for notice in &inspection.notices {
            show_notice(interaction, notice)?;
        }
    }
    interaction.blank()
}

fn show_audio(
    interaction: &mut impl Interaction,
    audio: &crate::source::InspectedAudio,
) -> io::Result<()> {
    interaction.present(UiLine::new().with(
        SemanticRole::Path,
        format!("  {}", audio.relative_path.display()),
    ))?;
    show_detail_field(
        interaction,
        "Audio",
        &format!(
            "{} · {} · {} Hz · {} channel(s)",
            audio.format,
            duration(audio.properties.duration),
            optional(audio.properties.sample_rate),
            optional(audio.properties.channels)
        ),
    )?;
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
    show_values(
        interaction,
        "MusicBrainz artist IDs",
        &audio.tags.artist_ids,
    )?;
    show_values(
        interaction,
        "MusicBrainz album-artist IDs",
        &audio.tags.album_artist_ids,
    )?;
    show_detail_field(
        interaction,
        "Compilation",
        audio
            .tags
            .compilation
            .map_or("(missing)", |value| if value { "yes" } else { "no" }),
    )?;
    show_tag(interaction, "Date", audio.tags.date.as_deref())?;
    show_detail_field(
        interaction,
        "Position",
        &format!(
            "disc {} of {}, track {} of {}",
            optional(audio.tags.disc),
            optional(audio.tags.disc_total),
            optional(audio.tags.track),
            optional(audio.tags.track_total)
        ),
    )?;
    show_detail_field(
        interaction,
        "Embedded artwork",
        &format!("{} picture(s), preserved", audio.tags.embedded_pictures),
    )?;
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
    interaction.warning(format!(
        "    Eventual filename correction: {} → {}",
        audio.relative_path.display(),
        destination.display()
    ))
}

fn show_artwork(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    if inspection.artwork.is_empty() {
        return Ok(());
    }
    interaction.heading("  Source artwork candidates")?;
    if inspection.selected_artwork.is_none() {
        interaction.warning("    No canonical sidecar selected yet.")?;
    }
    for (index, artwork) in inspection.artwork.iter().enumerate() {
        let is_selected = inspection.selected_artwork == Some(index);
        let selected = if is_selected { " (selected)" } else { "" };
        let role = if is_selected {
            SemanticRole::Selected
        } else {
            SemanticRole::Alternative
        };
        interaction.present(
            UiLine::new().with(role, format!("    {}{selected}", artwork_summary(artwork))),
        )?;
        if is_selected {
            let canonical = format!("cover.{}", artwork.format.canonical_extension());
            if artwork.relative_path.to_string_lossy() != canonical {
                interaction.prose(format!(
                    "      Eventual sidecar: {} → {canonical}",
                    artwork.relative_path.display()
                ))?;
            }
        } else {
            interaction.prose("      Preserved unchanged unless selected later")?;
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
    interaction.heading("  Ancillary files")?;
    for file in &inspection.ancillary {
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "    ")
                .with(SemanticRole::Path, file.relative_path.display().to_string())
                .with(
                    SemanticRole::Prose,
                    format!(" ({} bytes, preserved)", file.bytes),
                ),
        )?;
    }
    Ok(())
}

fn show_tag(
    interaction: &mut impl Interaction,
    label: &str,
    value: Option<&str>,
) -> io::Result<()> {
    show_detail_field(interaction, label, value.unwrap_or("(missing)"))
}

fn show_values(
    interaction: &mut impl Interaction,
    label: &str,
    values: &[String],
) -> io::Result<()> {
    show_detail_field(
        interaction,
        label,
        &if values.is_empty() {
            "(missing)".to_owned()
        } else {
            values.join("; ")
        },
    )
}

fn show_detail_field(
    interaction: &mut impl Interaction,
    label: &str,
    value: &str,
) -> io::Result<()> {
    interaction.present(
        UiLine::new()
            .with(SemanticRole::Prose, "    ")
            .with(SemanticRole::FieldName, label)
            .with(SemanticRole::Prose, ": ")
            .with(SemanticRole::Value, value),
    )
}
