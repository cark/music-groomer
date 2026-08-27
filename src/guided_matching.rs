use std::io;
use std::time::SystemTime;

use crate::artwork_viewer::ArtworkViewer;
use crate::matching::MatchPolicy;
use crate::matching_ui::{MetadataSelection, choose};
use crate::provider::{
    ArtworkLookup, ArtworkProvider, ArtworkResolver, LookupOrigin, MetadataProvider,
    MetadataResolver, ProviderCache, ProviderError, ProviderEvent, ProviderProgress, WaitReason,
    equivalent_groomed_result, source_inspection,
};
use crate::source::SourceInspection;
use crate::terminal::{Interaction, TextStyle};

pub struct GuidedMatchResult {
    pub metadata: MetadataSelection,
    pub artwork: ArtworkSelection,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkSelection {
    Source,
    CoverArtArchive(crate::provider::ProviderArtwork),
    None,
}

pub fn run<M: MetadataProvider, A: ArtworkProvider, V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    offline: bool,
    metadata_provider: M,
    artwork_provider: A,
    cache: ProviderCache,
    viewer: &mut V,
) -> io::Result<GuidedMatchResult> {
    let (inspection, search) = source_inspection(source);
    interaction.show("")?;
    if offline {
        show_styled(
            interaction,
            TextStyle::Heading,
            "Metadata lookup (offline: providers will not be contacted)",
        )?;
    } else {
        show_styled(
            interaction,
            TextStyle::Heading,
            "Looking for metadata (MusicBrainz can take a little while)",
        )?;
    }

    let mut metadata_resolver = MetadataResolver::new(metadata_provider, cache.clone());
    let lookup = {
        let mut progress = InteractionProgress(interaction);
        metadata_resolver.lookup(&search, offline, false, SystemTime::now(), &mut progress)
    };
    show_lookup_origin(interaction, lookup.origin)?;
    show_warnings(interaction, &lookup.warnings)?;
    let mut warnings = lookup.warnings;
    let prior_warnings = warnings.len();
    let candidates = resolve_missing_years(&inspection, lookup.candidates, &mut warnings);
    show_warnings(interaction, &warnings[prior_warnings..])?;
    let decision = MatchPolicy::default().decide(&inspection, candidates);
    let mut metadata = choose(interaction, &inspection, decision)?;
    if metadata == MetadataSelection::Cancelled {
        return Ok(GuidedMatchResult {
            metadata,
            artwork: ArtworkSelection::None,
            warnings,
        });
    }

    let mut artwork_resolver = ArtworkResolver::new(artwork_provider, cache);
    let mut archive_artwork = fetch_artwork(
        interaction,
        &mut artwork_resolver,
        &metadata,
        offline,
        false,
        &mut warnings,
    )?;
    let mut artwork = initial_artwork(source, archive_artwork.as_ref());
    warn_if_no_artwork(interaction, &artwork, &mut warnings)?;

    loop {
        show_preview(
            interaction,
            source,
            &metadata,
            &artwork,
            archive_artwork.as_ref(),
            &warnings,
        )?;
        let refresh = if offline {
            ""
        } else {
            "  [f] Refresh provider data"
        };
        let answer = interaction
            .ask(&format!(
                "Choose: [r] Review match  [a] Artwork{refresh}  [d] Done: "
            ))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "r" | "review" => review_match(interaction, &metadata)?,
            "a" | "artwork" => {
                artwork = choose_artwork(
                    interaction,
                    source,
                    archive_artwork.as_ref(),
                    artwork,
                    viewer,
                )?;
            }
            "f" | "refresh" if !offline => {
                let refreshed = {
                    let mut progress = InteractionProgress(interaction);
                    metadata_resolver.lookup(&search, false, true, SystemTime::now(), &mut progress)
                };
                show_lookup_origin(interaction, refreshed.origin)?;
                show_warnings(interaction, &refreshed.warnings)?;
                warnings.extend(refreshed.warnings.clone());
                let prior_warnings = warnings.len();
                let refreshed_candidates =
                    resolve_missing_years(&inspection, refreshed.candidates, &mut warnings);
                show_warnings(interaction, &warnings[prior_warnings..])?;
                let refreshed_selection = choose(
                    interaction,
                    &inspection,
                    MatchPolicy::default().decide(&inspection, refreshed_candidates),
                )?;
                if refreshed_selection == MetadataSelection::Cancelled {
                    interaction.show("Current preview kept unchanged.")?;
                } else if same_result(&metadata, &refreshed_selection) {
                    interaction.show("Provider data is current; the preview did not change.")?;
                } else if confirm_refreshed(interaction)? {
                    metadata = refreshed_selection;
                    archive_artwork = fetch_artwork(
                        interaction,
                        &mut artwork_resolver,
                        &metadata,
                        false,
                        false,
                        &mut warnings,
                    )?;
                    artwork = initial_artwork(source, archive_artwork.as_ref());
                    warn_if_no_artwork(interaction, &artwork, &mut warnings)?;
                } else {
                    interaction.show("Current preview kept unchanged.")?;
                }
            }
            "" => {}
            "d" | "done" | "q" | "quit" => break,
            _ => interaction.show("Please choose Review match, Artwork, Refresh, or Done.")?,
        }
    }

    Ok(GuidedMatchResult {
        metadata,
        artwork,
        warnings,
    })
}

fn resolve_missing_years(
    inspection: &crate::domain::Inspection,
    mut candidates: Vec<crate::domain::CandidateRelease>,
    warnings: &mut Vec<String>,
) -> Vec<crate::domain::CandidateRelease> {
    let source_year = inspection.tracks.first().and_then(|track| {
        let year = track.original_year?;
        inspection
            .tracks
            .iter()
            .all(|track| track.original_year == Some(year))
            .then_some(year)
    });
    let missing = candidates
        .iter()
        .any(|candidate| candidate.original_year.is_none());
    if missing {
        if let Some(year) = source_year {
            for candidate in &mut candidates {
                candidate.original_year.get_or_insert(year);
            }
            warnings.push(format!(
                "MusicBrainz lacked an original year for a candidate; source year {year} was preserved as unverified"
            ));
        } else {
            warnings.push(
                "MusicBrainz and the source do not provide an original year; it will remain absent"
                    .into(),
            );
        }
    }
    candidates
}

fn fetch_artwork<A: ArtworkProvider>(
    interaction: &mut impl Interaction,
    resolver: &mut ArtworkResolver<A>,
    metadata: &MetadataSelection,
    offline: bool,
    force_refresh: bool,
    warnings: &mut Vec<String>,
) -> io::Result<Option<crate::provider::ProviderArtwork>> {
    let MetadataSelection::Provider(selected) = metadata else {
        return Ok(None);
    };
    let Some(group) = selected.candidate.release_group_id.as_deref() else {
        warnings.push("Selected metadata has no release-group artwork identity".into());
        return Ok(None);
    };
    interaction.show("Checking Cover Art Archive for a canonical front cover...")?;
    let lookup = {
        let mut progress = InteractionProgress(interaction);
        resolver.lookup(group, offline, force_refresh, &mut progress)
    };
    show_artwork_origin(interaction, &lookup)?;
    show_warnings(interaction, &lookup.warnings)?;
    warnings.extend(lookup.warnings);
    Ok(lookup.artwork)
}

fn warn_if_no_artwork(
    interaction: &mut impl Interaction,
    artwork: &ArtworkSelection,
    warnings: &mut Vec<String>,
) -> io::Result<()> {
    if artwork == &ArtworkSelection::None
        && !warnings
            .iter()
            .any(|warning| warning == "No album artwork is available")
    {
        let warning = "No album artwork is available".to_owned();
        show_warnings(interaction, std::slice::from_ref(&warning))?;
        warnings.push(warning);
    }
    Ok(())
}

fn initial_artwork(
    source: &SourceInspection,
    archive: Option<&crate::provider::ProviderArtwork>,
) -> ArtworkSelection {
    if source.selected_artwork.is_some() {
        ArtworkSelection::Source
    } else if let Some(artwork) = archive {
        ArtworkSelection::CoverArtArchive(artwork.clone())
    } else {
        ArtworkSelection::None
    }
}

fn show_preview(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    metadata: &MetadataSelection,
    artwork: &ArtworkSelection,
    archive: Option<&crate::provider::ProviderArtwork>,
    warnings: &[String],
) -> io::Result<()> {
    show_styled(interaction, TextStyle::Heading, "\nmetadata preview")?;
    match metadata {
        MetadataSelection::Provider(selected) => {
            interaction.show(&format!("  Selected: {}", selected.candidate.human_label()))?;
            interaction.show("  Verification: MusicBrainz")?;
        }
        MetadataSelection::ExistingTags => {
            interaction.show("  Selected: existing source metadata")?;
            interaction.show("  Verification: unverified")?;
        }
        MetadataSelection::Cancelled => {}
    }
    interaction.show(&format!("  Artwork: {}", artwork_label(source, artwork)))?;
    if source.selected_artwork.is_some() && archive.is_some() {
        interaction.show("  Artwork alternative: Cover Art Archive 1200px front")?;
    }
    if warnings.is_empty() {
        interaction.show("  Warnings: none")?;
    } else {
        interaction.show(&format!("  Warnings: {} (shown above)", warnings.len()))?;
    }
    interaction.show("  No files were changed. Apply arrives in milestone 4.")
}

fn artwork_label(source: &SourceInspection, artwork: &ArtworkSelection) -> String {
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

fn choose_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    archive: Option<&crate::provider::ProviderArtwork>,
    current: ArtworkSelection,
    viewer: &mut V,
) -> io::Result<ArtworkSelection> {
    match (source.selected_artwork.is_some(), archive) {
        (true, Some(archive)) => loop {
            let answer = interaction.ask(
                "Artwork: [1] Keep source cover (default)  [2] Use Cover Art Archive front  [v] View current  [b] Back: ",
            )?;
            match answer.to_ascii_lowercase().as_str() {
                "1" => return Ok(ArtworkSelection::Source),
                "2" => return Ok(ArtworkSelection::CoverArtArchive(archive.clone())),
                "v" | "view" => view_artwork(interaction, source, &current, viewer)?,
                "" | "b" | "back" => return Ok(current),
                _ => interaction.show("Please choose Source, Cover Art Archive, or Back.")?,
            }
        },
        (true, None) => {
            view_artwork(interaction, source, &current, viewer)?;
            Ok(current)
        }
        (false, Some(_)) => {
            view_artwork(interaction, source, &current, viewer)?;
            Ok(current)
        }
        (false, None) => {
            interaction.show("No album artwork is available.")?;
            Ok(current)
        }
    }
}

fn view_artwork<V: ArtworkViewer>(
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
        Ok(()) => interaction.show("Opened the selected artwork in the system image viewer."),
        Err(error) => show_styled(
            interaction,
            TextStyle::Error,
            &format!("Could not view artwork: {error}"),
        ),
    }
}

fn review_match(
    interaction: &mut impl Interaction,
    metadata: &MetadataSelection,
) -> io::Result<()> {
    match metadata {
        MetadataSelection::Provider(selected) => {
            interaction.show("\nWhy this match:")?;
            for reason in &selected.reasons {
                interaction.show(&format!("  - {}", reason.summary))?;
            }
            interaction.show(&format!(
                "  MusicBrainz release group: {}",
                selected
                    .candidate
                    .release_group_id
                    .as_deref()
                    .unwrap_or("unknown")
            ))?;
            interaction.show(&format!("  Track mappings: {}", selected.mappings.len()))
        }
        MetadataSelection::ExistingTags => {
            interaction.show("Existing tags are internally coherent but not provider-verified.")
        }
        MetadataSelection::Cancelled => Ok(()),
    }
}

fn same_result(left: &MetadataSelection, right: &MetadataSelection) -> bool {
    match (left, right) {
        (MetadataSelection::Provider(left), MetadataSelection::Provider(right)) => {
            equivalent_groomed_result(&left.candidate, &right.candidate)
        }
        (MetadataSelection::ExistingTags, MetadataSelection::ExistingTags) => true,
        _ => false,
    }
}

fn confirm_refreshed(interaction: &mut impl Interaction) -> io::Result<bool> {
    loop {
        let answer = interaction.ask("Use the materially changed refreshed metadata? [y/N]: ")?;
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.show("Please answer Yes or No.")?,
        }
    }
}

fn show_lookup_origin(interaction: &mut impl Interaction, origin: LookupOrigin) -> io::Result<()> {
    let message = match origin {
        LookupOrigin::Live => "MusicBrainz lookup completed.",
        LookupOrigin::Refreshed => "MusicBrainz data refreshed.",
        LookupOrigin::FreshCache => "Fresh metadata cache hit; MusicBrainz was not contacted.",
        LookupOrigin::StaleFallback => "Using stale cached metadata after refresh failure.",
        LookupOrigin::OfflineStaleCache => "Using stale cached metadata in offline mode.",
        LookupOrigin::OfflineMiss => "No cached metadata is available in offline mode.",
        LookupOrigin::ProviderUnavailable => "MusicBrainz metadata is unavailable.",
    };
    interaction.show(message)
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
        ArtworkLookupOrigin::CacheFallback => "Using cached artwork after refresh failure.",
        ArtworkLookupOrigin::OfflineCache => "Using artwork cache in offline mode.",
        ArtworkLookupOrigin::ProviderUnavailable => "Cover Art Archive artwork is unavailable.",
    };
    interaction.show(message)
}

fn show_warnings(interaction: &mut impl Interaction, warnings: &[String]) -> io::Result<()> {
    for warning in warnings {
        show_styled(
            interaction,
            TextStyle::Warning,
            &format!("Warning: {warning}"),
        )?;
    }
    Ok(())
}

fn show_styled(interaction: &mut impl Interaction, style: TextStyle, text: &str) -> io::Result<()> {
    let text = interaction.styled(style, text);
    interaction.show(&text)
}

struct InteractionProgress<'a, I>(&'a mut I);

impl<I: Interaction> ProviderProgress for InteractionProgress<'_, I> {
    fn event(&mut self, event: ProviderEvent) -> Result<(), ProviderError> {
        let message = match event {
            ProviderEvent::Requesting(operation) => format!("  {operation}..."),
            ProviderEvent::Waiting {
                seconds,
                reason: WaitReason::RateLimit,
            } => format!("  Waiting {seconds}s for MusicBrainz's rate limit..."),
            ProviderEvent::Waiting {
                seconds,
                reason: WaitReason::Retry,
            } => format!("  Provider unavailable; retrying in {seconds}s (Ctrl-C exits)..."),
        };
        self.0
            .show(&message)
            .map_err(|error| ProviderError::Progress(error.to_string()))
    }
}

#[cfg(test)]
mod tests;
