use std::io;
use std::time::SystemTime;

use crate::artwork_viewer::ArtworkViewer;
use crate::matching::MatchPolicy;
use crate::matching::RankedCandidate;
use crate::matching_ui::{MetadataSelection, choose, revise};
use crate::provider::{
    ArtworkLookup, ArtworkProvider, ArtworkResolver, LookupOrigin, MetadataProvider,
    MetadataResolver, ProviderCache, ProviderError, ProviderEvent, ProviderProgress, WaitReason,
    equivalent_groomed_result, source_inspection,
};
use crate::source::SourceInspection;
use crate::terminal::{Interaction, TextStyle};

pub struct GuidedMatchResult {
    pub metadata: MetadataSelection,
    pub metadata_provenance: MetadataProvenance,
    pub candidates: Vec<RankedCandidate>,
    pub artwork: ArtworkSelection,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataProvenance {
    MusicBrainz,
    MusicBrainzWithSourceYear(u16),
    ExistingTags { artwork_via_source_id: bool },
    None,
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
            "Checking metadata and provider cache",
        )?;
    }

    let mut metadata_resolver = MetadataResolver::new(metadata_provider, cache.clone());
    let lookup = {
        let mut progress = InteractionProgress(interaction);
        metadata_resolver.lookup(&search, offline, false, SystemTime::now(), &mut progress)
    };
    show_lookup_origin(interaction, lookup.origin)?;
    show_warnings(interaction, &lookup.warnings)?;
    let mut warnings = source_warnings(source);
    warnings.extend(identifier_warnings(&inspection));
    warnings.extend(lookup.warnings);
    deduplicate(&mut warnings);
    let decision = MatchPolicy::default().decide(&inspection, lookup.candidates);
    let mut candidates = decision.candidates().to_vec();
    let mut metadata = choose(interaction, &inspection, decision)?;
    if metadata == MetadataSelection::Cancelled {
        return Ok(GuidedMatchResult {
            metadata,
            metadata_provenance: MetadataProvenance::None,
            candidates,
            artwork: ArtworkSelection::None,
            warnings,
        });
    }
    let mut source_year_fallback = add_year_fallback(&inspection, &mut metadata, &mut warnings);

    let mut artwork_resolver = ArtworkResolver::new(artwork_provider, cache);
    let mut archive_artwork = fetch_artwork(
        interaction,
        &mut artwork_resolver,
        &inspection,
        &metadata,
        offline,
        false,
        &mut warnings,
    )?;
    let mut artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
    warn_if_no_artwork(
        interaction,
        &artwork,
        archive_artwork.as_ref(),
        &mut warnings,
    )?;

    loop {
        show_preview(
            interaction,
            source,
            &metadata,
            &artwork,
            archive_artwork.as_ref(),
            &warnings,
            source_year_fallback,
        )?;
        let refresh = if offline {
            ""
        } else {
            "  [f] Refresh provider data and artwork"
        };
        let answer = interaction
            .ask(&format!(
                "Choose: [r] Review  [a] Artwork{refresh}  [d] Done: "
            ))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "r" | "review" => {
                let changed = review(
                    interaction,
                    source,
                    &inspection,
                    &candidates,
                    &mut metadata,
                    &warnings,
                )?;
                if changed {
                    source_year_fallback =
                        add_year_fallback(&inspection, &mut metadata, &mut warnings);
                    archive_artwork = fetch_artwork(
                        interaction,
                        &mut artwork_resolver,
                        &inspection,
                        &metadata,
                        offline,
                        false,
                        &mut warnings,
                    )?;
                    artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
                    warn_if_no_artwork(
                        interaction,
                        &artwork,
                        archive_artwork.as_ref(),
                        &mut warnings,
                    )?;
                }
            }
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
                deduplicate(&mut warnings);
                let refreshed_decision =
                    MatchPolicy::default().decide(&inspection, refreshed.candidates);
                candidates = refreshed_decision.candidates().to_vec();
                let refreshed_selection = choose(interaction, &inspection, refreshed_decision)?;
                let metadata_replaced = if refreshed_selection == MetadataSelection::Cancelled {
                    interaction.show("Current preview kept unchanged.")?;
                    false
                } else if same_result(&metadata, &refreshed_selection) {
                    interaction.show("Provider data is current; the preview did not change.")?;
                    metadata = refreshed_selection;
                    true
                } else if confirm_refreshed(interaction)? {
                    metadata = refreshed_selection;
                    true
                } else {
                    interaction.show("Current preview kept unchanged.")?;
                    false
                };
                if metadata_replaced {
                    source_year_fallback =
                        add_year_fallback(&inspection, &mut metadata, &mut warnings);
                }
                let refreshed_artwork = fetch_artwork(
                    interaction,
                    &mut artwork_resolver,
                    &inspection,
                    &metadata,
                    false,
                    true,
                    &mut warnings,
                )?;
                if archive_artwork != refreshed_artwork {
                    show_artwork_change(
                        interaction,
                        archive_artwork.as_ref(),
                        refreshed_artwork.as_ref(),
                    )?;
                    if confirm_refreshed_artwork(interaction)? {
                        archive_artwork = refreshed_artwork;
                        if matches!(artwork, ArtworkSelection::CoverArtArchive(_))
                            || source.selected_artwork.is_none()
                        {
                            artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
                        }
                    } else {
                        interaction.show("Current artwork choice kept unchanged.")?;
                    }
                } else {
                    interaction.show("Cover Art Archive artwork is current.")?;
                }
                warn_if_no_artwork(
                    interaction,
                    &artwork,
                    archive_artwork.as_ref(),
                    &mut warnings,
                )?;
            }
            "" => {}
            "d" | "done" | "q" | "quit" => break,
            _ => interaction.show("Please choose Review, Artwork, Refresh, or Done.")?,
        }
    }

    let metadata_provenance = match &metadata {
        MetadataSelection::Provider(_) => source_year_fallback.map_or(
            MetadataProvenance::MusicBrainz,
            MetadataProvenance::MusicBrainzWithSourceYear,
        ),
        MetadataSelection::ExistingTags => MetadataProvenance::ExistingTags {
            artwork_via_source_id: common_release_group_id(&inspection).is_some(),
        },
        MetadataSelection::Cancelled => MetadataProvenance::None,
    };
    Ok(GuidedMatchResult {
        metadata,
        metadata_provenance,
        candidates,
        artwork,
        warnings,
    })
}

fn add_year_fallback(
    inspection: &crate::domain::Inspection,
    metadata: &mut MetadataSelection,
    warnings: &mut Vec<String>,
) -> Option<u16> {
    let source_year = inspection.tracks.first().and_then(|track| {
        let year = track.original_year?;
        inspection
            .tracks
            .iter()
            .all(|track| track.original_year == Some(year))
            .then_some(year)
    });
    let MetadataSelection::Provider(selected) = metadata else {
        return None;
    };
    if selected.candidate.original_year.is_none() {
        if let Some(year) = source_year {
            selected.candidate.original_year = Some(year);
            warnings.push(format!(
                "Selected MusicBrainz metadata has no original year; source year {year} is used as an unverified fallback"
            ));
            deduplicate(warnings);
            return Some(year);
        } else {
            warnings.push(
                "MusicBrainz and the source do not provide an original year; it will remain absent"
                    .into(),
            );
        }
    }
    deduplicate(warnings);
    None
}

fn fetch_artwork<A: ArtworkProvider>(
    interaction: &mut impl Interaction,
    resolver: &mut ArtworkResolver<A>,
    inspection: &crate::domain::Inspection,
    metadata: &MetadataSelection,
    offline: bool,
    force_refresh: bool,
    warnings: &mut Vec<String>,
) -> io::Result<Option<crate::provider::ProviderArtwork>> {
    let group = match metadata {
        MetadataSelection::Provider(selected) => selected.candidate.release_group_id.as_deref(),
        MetadataSelection::ExistingTags => common_release_group_id(inspection),
        MetadataSelection::Cancelled => None,
    };
    let Some(group) = group else {
        warnings.push("Selected metadata has no release-group artwork identity".into());
        return Ok(None);
    };
    interaction.show("Checking Cover Art Archive for a canonical front cover...")?;
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
    warnings.extend(lookup.warnings);
    deduplicate(warnings);
    Ok(lookup.artwork)
}

fn warn_if_no_artwork(
    interaction: &mut impl Interaction,
    artwork: &ArtworkSelection,
    archive: Option<&crate::provider::ProviderArtwork>,
    warnings: &mut Vec<String>,
) -> io::Result<()> {
    if artwork == &ArtworkSelection::None
        && archive.is_none()
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
    metadata: &MetadataSelection,
    archive: Option<&crate::provider::ProviderArtwork>,
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

fn show_preview(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    metadata: &MetadataSelection,
    artwork: &ArtworkSelection,
    archive: Option<&crate::provider::ProviderArtwork>,
    warnings: &[String],
    source_year_fallback: Option<u16>,
) -> io::Result<()> {
    show_styled(interaction, TextStyle::Heading, "\nmetadata preview")?;
    match metadata {
        MetadataSelection::Provider(selected) => {
            interaction.show(&format!("  Selected: {}", selected.candidate.human_label()))?;
            interaction.show("  Verification: MusicBrainz")?;
            if let Some(year) = source_year_fallback {
                interaction.show(&format!(
                    "  Year provenance: source tags ({year}, unverified fallback)"
                ))?;
            } else {
                interaction.show("  Year provenance: MusicBrainz")?;
            }
        }
        MetadataSelection::ExistingTags => {
            interaction.show("  Selected: existing source metadata")?;
            interaction.show("  Verification: unverified")?;
            if common_release_group_id(&source_inspection(source).0).is_some() {
                interaction.show(
                    "  Artwork provenance: Cover Art Archive via existing source ID; metadata remains unverified",
                )?;
            }
        }
        MetadataSelection::Cancelled => {}
    }
    interaction.show(&format!("  Artwork: {}", artwork_label(source, artwork)))?;
    if archive.is_some() && !matches!(artwork, ArtworkSelection::CoverArtArchive(_)) {
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
        (false, Some(archive)) => loop {
            let answer = interaction.ask(
                "Artwork: [2] Use Cover Art Archive front  [v] View archive front  [b] Back: ",
            )?;
            match answer.to_ascii_lowercase().as_str() {
                "2" => return Ok(ArtworkSelection::CoverArtArchive(archive.clone())),
                "v" | "view" => view_artwork(
                    interaction,
                    source,
                    &ArtworkSelection::CoverArtArchive(archive.clone()),
                    viewer,
                )?,
                "" | "b" | "back" => return Ok(current),
                _ => interaction.show("Please choose Use, View, or Back.")?,
            }
        },
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

fn review(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    inspection: &crate::domain::Inspection,
    candidates: &[RankedCandidate],
    metadata: &mut MetadataSelection,
    warnings: &[String],
) -> io::Result<bool> {
    loop {
        let answer = interaction
            .ask("Review: [s] Source files and tags  [m] Metadata  [w] Warnings  [b] Back: ")?;
        match answer.to_ascii_lowercase().as_str() {
            "s" | "source" => show_source_review(interaction, source)?,
            "m" | "metadata" => {
                let revised = revise(interaction, inspection, candidates, metadata)?;
                let changed = selection_key(&revised) != selection_key(metadata);
                *metadata = revised;
                return Ok(changed);
            }
            "w" | "warnings" => {
                if warnings.is_empty() {
                    interaction.show("No warnings.")?;
                } else {
                    show_warnings(interaction, warnings)?;
                }
            }
            "" | "b" | "back" => return Ok(false),
            _ => interaction.show("Please choose Source, Metadata, Warnings, or Back.")?,
        }
    }
}

fn show_source_review(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
) -> io::Result<()> {
    interaction.show("\nSource files and tags:")?;
    for audio in &source.audio {
        interaction.show(&format!("  {}", audio.relative_path.display()))?;
        interaction.show(&format!(
            "    title: {} | artist: {} | album: {} | album artist: {} | disc-track: {}-{}",
            audio.tags.title.as_deref().unwrap_or("?"),
            audio.tags.artist.as_deref().unwrap_or("?"),
            audio.tags.album.as_deref().unwrap_or("?"),
            audio.tags.album_artist.as_deref().unwrap_or("?"),
            audio
                .tags
                .disc
                .map_or_else(|| "?".into(), |value| value.to_string()),
            audio
                .tags
                .track
                .map_or_else(|| "?".into(), |value| value.to_string()),
        ))?;
    }
    Ok(())
}

fn selection_key(metadata: &MetadataSelection) -> Option<&str> {
    match metadata {
        MetadataSelection::Provider(selected) => Some(&selected.candidate.provider_key),
        MetadataSelection::ExistingTags => Some("existing-tags"),
        MetadataSelection::Cancelled => None,
    }
}

fn source_warnings(source: &SourceInspection) -> Vec<String> {
    source
        .notices
        .iter()
        .filter(|notice| notice.severity == crate::source::NoticeSeverity::Warning)
        .map(|notice| match &notice.path {
            Some(path) => format!("{}: {}", path.display(), notice.message),
            None => notice.message.clone(),
        })
        .collect()
}

fn identifier_warnings(inspection: &crate::domain::Inspection) -> Vec<String> {
    let mut warnings = Vec::new();
    let release_group_count = inspection
        .tracks
        .iter()
        .filter(|track| track.release_group_id.is_some())
        .count();
    let release_groups = inspection
        .tracks
        .iter()
        .filter_map(|track| track.release_group_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    if release_groups.len() > 1
        || (release_group_count > 0 && release_group_count != inspection.tracks.len())
    {
        warnings.push(
            "Source MusicBrainz release-group identifiers are inconsistent; using textual discovery instead"
                .into(),
        );
    }
    let album_artist_ids = inspection
        .tracks
        .iter()
        .map(|track| track.album_artist_ids.as_slice())
        .collect::<std::collections::BTreeSet<_>>();
    if album_artist_ids.len() > 1 {
        warnings.push(
            "Source MusicBrainz album-artist identifiers are inconsistent; using artist names instead"
                .into(),
        );
    }
    warnings
}

fn common_release_group_id(inspection: &crate::domain::Inspection) -> Option<&str> {
    let first = inspection.tracks.first()?.release_group_id.as_deref()?;
    inspection
        .tracks
        .iter()
        .all(|track| track.release_group_id.as_deref() == Some(first))
        .then_some(first)
}

fn deduplicate(warnings: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));
}

fn show_artwork_change(
    interaction: &mut impl Interaction,
    previous: Option<&crate::provider::ProviderArtwork>,
    refreshed: Option<&crate::provider::ProviderArtwork>,
) -> io::Result<()> {
    interaction.show("Cover Art Archive artwork changed:")?;
    interaction.show(&format!("  Previous: {}", provider_artwork_label(previous)))?;
    interaction.show(&format!(
        "  Refreshed: {}",
        provider_artwork_label(refreshed)
    ))
}

fn provider_artwork_label(artwork: Option<&crate::provider::ProviderArtwork>) -> String {
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

fn confirm_refreshed_artwork(interaction: &mut impl Interaction) -> io::Result<bool> {
    loop {
        let answer = interaction.ask("Use the refreshed archive artwork? [y/N]: ")?;
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.show("Please answer Yes or No.")?,
        }
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
        ArtworkLookupOrigin::ConfirmedAbsentCache => {
            "Artwork cache confirms Cover Art Archive has no front image; it was not contacted."
        }
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
