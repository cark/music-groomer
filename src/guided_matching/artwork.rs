use std::io;
use std::time::SystemTime;

use crate::artwork_viewer::ArtworkViewer;
use crate::matching_ui::MetadataSelection;
use crate::provider::{ArtworkLookup, ArtworkProvider, ArtworkResolver, ProviderArtwork};
use crate::source::SourceInspection;
use crate::terminal::{Interaction, UiLine};

use super::warnings::WarningState;
use super::{InteractionProgress, common_release_group_id, show_warnings};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkSelection {
    Source,
    CoverArtArchive(ProviderArtwork),
    None,
}

pub(super) struct ArtworkFetch {
    pub(super) artwork: Option<ProviderArtwork>,
    pub(super) warnings: Vec<String>,
}

pub(super) fn set_artwork_warnings(
    interaction: &mut impl Interaction,
    warning_state: &mut WarningState,
    warnings: Vec<String>,
    artwork: &ArtworkSelection,
    archive: Option<&ProviderArtwork>,
) -> io::Result<()> {
    const NO_ARTWORK: &str = "No album artwork is available";
    let previously_missing = warning_state
        .current()
        .iter()
        .any(|warning| warning == NO_ARTWORK);
    warning_state.set_artwork(warnings, artwork, archive);
    let now_missing = warning_state
        .current()
        .iter()
        .any(|warning| warning == NO_ARTWORK);
    if now_missing && !previously_missing {
        show_warnings(interaction, &[NO_ARTWORK.into()])?;
    }
    Ok(())
}

pub(super) fn fetch_artwork<A: ArtworkProvider>(
    interaction: &mut impl Interaction,
    resolver: &mut ArtworkResolver<A>,
    inspection: &crate::domain::Inspection,
    metadata: &MetadataSelection,
    offline: bool,
    force_refresh: bool,
) -> io::Result<ArtworkFetch> {
    let group = match metadata {
        MetadataSelection::Provider(selected) => selected.candidate.release_group_id.as_deref(),
        MetadataSelection::ExistingTags => common_release_group_id(inspection),
        MetadataSelection::Cancelled => None,
    };
    let Some(group) = group else {
        return Ok(ArtworkFetch {
            artwork: None,
            warnings: vec!["Selected metadata has no release-group artwork identity".into()],
        });
    };
    interaction.section_heading("Checking album artwork")?;
    let lookup = {
        let mut progress = InteractionProgress(interaction);
        resolver.lookup(
            group,
            offline,
            force_refresh,
            SystemTime::now(),
            &mut progress,
        )
    };
    show_artwork_origin(interaction, &lookup)?;
    show_warnings(interaction, &lookup.warnings)?;
    Ok(ArtworkFetch {
        artwork: lookup.artwork,
        warnings: lookup.warnings,
    })
}

pub(super) fn initial_artwork(
    source: &SourceInspection,
    metadata: &MetadataSelection,
    archive: Option<&ProviderArtwork>,
) -> ArtworkSelection {
    if source.selected_artwork.is_some() {
        ArtworkSelection::Source
    } else if let Some(artwork) = archive
        && matches!(metadata, MetadataSelection::Provider(_))
    {
        ArtworkSelection::CoverArtArchive(artwork.clone())
    } else {
        ArtworkSelection::None
    }
}

pub(super) fn artwork_label(source: &SourceInspection, artwork: &ArtworkSelection) -> String {
    match artwork {
        ArtworkSelection::Source => source
            .selected_artwork
            .and_then(|index| source.artwork.get(index))
            .map_or_else(
                || "selected source cover".to_owned(),
                |artwork| format!("source {}", artwork.relative_path.display()),
            ),
        ArtworkSelection::CoverArtArchive(artwork) => format!(
            "Cover Art Archive front ({} {}x{})",
            artwork.format, artwork.dimensions.0, artwork.dimensions.1
        ),
        ArtworkSelection::None => "none available".into(),
    }
}

pub(super) fn choose_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    archive: Option<&ProviderArtwork>,
    current: ArtworkSelection,
    viewer: &mut V,
) -> io::Result<ArtworkSelection> {
    match (source.selected_artwork.is_some(), archive) {
        (true, Some(archive)) => loop {
            let answer = interaction.prompt(UiLine::menu_prompt(
                "Artwork: [1] Keep source cover (default)  [2] Use Cover Art Archive front  [v] View current  [b] Back: ",
            ))?;
            match answer.to_ascii_lowercase().as_str() {
                "1" => return Ok(ArtworkSelection::Source),
                "2" => return Ok(ArtworkSelection::CoverArtArchive(archive.clone())),
                "v" | "view" => view_artwork(interaction, source, &current, viewer)?,
                "" | "b" | "back" => return Ok(current),
                _ => interaction.error("Please choose Source, Cover Art Archive, or Back.")?,
            }
        },
        (true, None) => {
            view_artwork(interaction, source, &current, viewer)?;
            Ok(current)
        }
        (false, Some(archive)) => loop {
            let answer = interaction.prompt(UiLine::menu_prompt(
                "Artwork: [2] Use Cover Art Archive front  [v] View archive front  [b] Back: ",
            ))?;
            match answer.to_ascii_lowercase().as_str() {
                "2" => return Ok(ArtworkSelection::CoverArtArchive(archive.clone())),
                "v" | "view" => view_artwork(
                    interaction,
                    source,
                    &ArtworkSelection::CoverArtArchive(archive.clone()),
                    viewer,
                )?,
                "" | "b" | "back" => return Ok(current),
                _ => interaction.error("Please choose Use, View, or Back.")?,
            }
        },
        (false, None) => {
            interaction.warning("No album artwork is available.")?;
            Ok(current)
        }
    }
}

pub(super) fn view_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    artwork: &ArtworkSelection,
    viewer: &mut V,
) -> io::Result<()> {
    let result = match artwork {
        ArtworkSelection::Source => source
            .selected_artwork
            .and_then(|index| source.artwork.get(index))
            .ok_or_else(|| "the selected source cover is unavailable".to_owned())
            .and_then(|artwork| {
                let root = if source.source.is_dir() {
                    source.source.clone()
                } else {
                    source
                        .source
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .to_owned()
                };
                viewer
                    .view_path(&root.join(&artwork.relative_path))
                    .map_err(|error| error.to_string())
            }),
        ArtworkSelection::CoverArtArchive(artwork) => viewer
            .view_download(artwork)
            .map_err(|error| error.to_string()),
        ArtworkSelection::None => Err("no artwork is available to view".into()),
    };
    match result {
        Ok(()) => interaction.success("Opened the selected artwork in the system image viewer."),
        Err(error) => interaction.error(format!("Could not view artwork: {error}")),
    }
}

pub(super) fn show_artwork_change(
    interaction: &mut impl Interaction,
    previous: Option<&ProviderArtwork>,
    refreshed: Option<&ProviderArtwork>,
) -> io::Result<()> {
    interaction.heading("Cover Art Archive artwork changed")?;
    interaction.field("Previous", provider_artwork_label(previous))?;
    interaction.field("Refreshed", provider_artwork_label(refreshed))
}

pub(super) fn confirm_refreshed_artwork(interaction: &mut impl Interaction) -> io::Result<bool> {
    loop {
        let answer = interaction.prompt(UiLine::menu_prompt(
            "Use the refreshed archive artwork? [y/N]: ",
        ))?;
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer Yes or No.")?,
        }
    }
}

fn provider_artwork_label(artwork: Option<&ProviderArtwork>) -> String {
    artwork.map_or_else(
        || "none".into(),
        |artwork| {
            format!(
                "{} {}x{}",
                artwork.format, artwork.dimensions.0, artwork.dimensions.1
            )
        },
    )
}

fn show_artwork_origin(
    interaction: &mut impl Interaction,
    lookup: &ArtworkLookup,
) -> io::Result<()> {
    use crate::provider::ArtworkLookupOrigin;
    let message = match lookup.origin {
        ArtworkLookupOrigin::Live => "Cover Art Archive lookup completed.",
        ArtworkLookupOrigin::Refreshed => "Cover Art Archive artwork refreshed.",
        ArtworkLookupOrigin::Cache => "Artwork cache hit; Cover Art Archive was not contacted.",
        ArtworkLookupOrigin::ConfirmedAbsentCache => {
            "Artwork cache confirms Cover Art Archive has no front image; it was not contacted."
        }
        ArtworkLookupOrigin::CacheFallback => "Using cached artwork after refresh failure.",
        ArtworkLookupOrigin::OfflineCache => "Using artwork cache in offline mode.",
        ArtworkLookupOrigin::ProviderUnavailable => "Cover Art Archive artwork is unavailable.",
    };
    interaction.success(message)
}
