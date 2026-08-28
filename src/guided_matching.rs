use std::io;
use std::time::SystemTime;

use crate::artwork_viewer::ArtworkViewer;
use crate::fingerprint::{AudioFingerprinter, FingerprintError, FingerprintProgress};
use crate::identification::FingerprintEvidence;
use crate::matching::RankedCandidate;
use crate::matching_ui::{MetadataSelection, choose, revise};
use crate::plan::MatchSelection;
use crate::provider::{
    AcoustIdLookupOrigin, AcoustIdProvider, ArtworkProvider, ArtworkResolver, LookupOrigin,
    MetadataProvider, MetadataResolver, ProviderCache, ProviderError, ProviderEvent,
    ProviderProgress, WaitReason, equivalent_groomed_result, source_inspection,
};
use crate::source::SourceInspection;
use crate::terminal::{Interaction, SemanticRole, UiLine};

mod artwork;
mod identification;
mod warnings;

pub use artwork::ArtworkSelection;
use artwork::{
    artwork_label, choose_artwork, confirm_refreshed_artwork, fetch_artwork, initial_artwork,
    set_artwork_warnings, show_artwork_change,
};
use identification::{IdentificationAdapters, identify};
use warnings::{WarningState, selection_year_warnings};

pub struct GuidedMatchResult {
    pub metadata: MetadataSelection,
    pub metadata_provenance: MetadataProvenance,
    pub candidates: Vec<RankedCandidate>,
    pub artwork: ArtworkSelection,
    pub archive_artwork: Option<crate::provider::ProviderArtwork>,
    pub identification: Option<FingerprintEvidence>,
    pub warnings: Vec<String>,
    pub match_selection: MatchSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataProvenance {
    MusicBrainz,
    MusicBrainzWithSourceYear(u16),
    MusicBrainzWithFingerprint,
    MusicBrainzWithFingerprintAndSourceYear(u16),
    ExistingTags { artwork_via_source_id: bool },
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
    run_inner(
        interaction,
        source,
        offline,
        RunProviders {
            metadata: metadata_provider,
            artwork: artwork_provider,
            identification: None,
        },
        cache,
        viewer,
    )
}

pub struct GuidedProviders<M, A, F, I> {
    metadata: M,
    artwork: A,
    fingerprinter: F,
    acoustid: I,
}

impl<M, A, F, I> GuidedProviders<M, A, F, I> {
    pub fn new(metadata: M, artwork: A, fingerprinter: F, acoustid: I) -> Self {
        Self {
            metadata,
            artwork,
            fingerprinter,
            acoustid,
        }
    }
}

pub fn run_with_identification<
    M: MetadataProvider,
    A: ArtworkProvider,
    V: ArtworkViewer,
    F: AudioFingerprinter,
    I: AcoustIdProvider,
>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    offline: bool,
    providers: GuidedProviders<M, A, F, I>,
    cache: ProviderCache,
    viewer: &mut V,
) -> io::Result<GuidedMatchResult> {
    let GuidedProviders {
        metadata,
        artwork,
        mut fingerprinter,
        mut acoustid,
    } = providers;
    run_inner(
        interaction,
        source,
        offline,
        RunProviders {
            metadata,
            artwork,
            identification: Some(IdentificationAdapters {
                fingerprinter: &mut fingerprinter,
                acoustid_provider: &mut acoustid,
            }),
        },
        cache,
        viewer,
    )
}

struct RunProviders<'a, M, A> {
    metadata: M,
    artwork: A,
    identification: Option<IdentificationAdapters<'a>>,
}

fn run_inner<M: MetadataProvider, A: ArtworkProvider, V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    offline: bool,
    providers: RunProviders<'_, M, A>,
    cache: ProviderCache,
    viewer: &mut V,
) -> io::Result<GuidedMatchResult> {
    let RunProviders {
        metadata,
        artwork,
        identification: mut identification_adapters,
    } = providers;
    let (inspection, search) = source_inspection(source);
    if offline {
        interaction
            .section_heading("Metadata lookup (offline: providers will not be contacted)")?;
    } else {
        interaction.section_heading("Checking metadata and provider cache")?;
    }

    let mut metadata_resolver = MetadataResolver::new(metadata, cache.clone());
    let lookup = {
        let mut progress = InteractionProgress(interaction);
        metadata_resolver.lookup(&search, offline, false, SystemTime::now(), &mut progress)
    };
    show_lookup_origin(interaction, lookup.origin)?;
    show_warnings(interaction, &lookup.warnings)?;
    let mut warning_state = WarningState::new(source, &inspection);
    warning_state.set_metadata(lookup.warnings);
    let identified = identify(
        interaction,
        source,
        &inspection,
        &search,
        lookup.candidates,
        &mut metadata_resolver,
        &cache,
        identification_adapters.as_mut(),
        offline,
        false,
    )?;
    warning_state.set_identification(identified.warnings);
    let mut identification = identified.evidence;
    let decision = identified.decision;
    let mut match_selection = match &decision {
        crate::matching::MatchDecision::Selected { .. } => MatchSelection::Automatic,
        crate::matching::MatchDecision::NeedsChoice(_) => MatchSelection::UserChosen,
        crate::matching::MatchDecision::NoUsableMatch(_) => MatchSelection::ExistingTags,
    };
    let mut candidates = decision.candidates().to_vec();
    let mut metadata = choose(interaction, &inspection, decision)?;
    if metadata == MetadataSelection::Cancelled {
        return Ok(GuidedMatchResult {
            metadata,
            metadata_provenance: MetadataProvenance::None,
            candidates,
            artwork: ArtworkSelection::None,
            archive_artwork: None,
            identification,
            warnings: warning_state.current(),
            match_selection,
        });
    }
    let (mut source_year_fallback, selection_warnings) =
        selection_year_warnings(&inspection, &mut metadata);
    warning_state.set_selection(selection_warnings);

    let mut artwork_resolver = ArtworkResolver::new(artwork, cache.clone());
    let fetched_artwork = fetch_artwork(
        interaction,
        &mut artwork_resolver,
        &inspection,
        &metadata,
        offline,
        false,
    )?;
    let mut archive_artwork = fetched_artwork.artwork;
    let mut artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
    set_artwork_warnings(
        interaction,
        &mut warning_state,
        fetched_artwork.warnings,
        &artwork,
        archive_artwork.as_ref(),
    )?;

    loop {
        let warnings = warning_state.current();
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
            .prompt(UiLine::menu_prompt(format!(
                "Choose: [r] Review  [a] Artwork{refresh}  [d] Done: "
            )))?
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
                    identification.as_ref(),
                )?;
                if changed {
                    match_selection = match metadata {
                        MetadataSelection::Provider(_) => MatchSelection::UserChosen,
                        MetadataSelection::ExistingTags => MatchSelection::ExistingTags,
                        MetadataSelection::Cancelled => match_selection,
                    };
                    let (fallback, selection_warnings) =
                        selection_year_warnings(&inspection, &mut metadata);
                    source_year_fallback = fallback;
                    warning_state.set_selection(selection_warnings);
                    let fetched = fetch_artwork(
                        interaction,
                        &mut artwork_resolver,
                        &inspection,
                        &metadata,
                        offline,
                        false,
                    )?;
                    archive_artwork = fetched.artwork;
                    artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
                    set_artwork_warnings(
                        interaction,
                        &mut warning_state,
                        fetched.warnings,
                        &artwork,
                        archive_artwork.as_ref(),
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
                interaction.section_heading("Refreshing provider data and artwork")?;
                let refreshed = {
                    let mut progress = InteractionProgress(interaction);
                    metadata_resolver.lookup(&search, false, true, SystemTime::now(), &mut progress)
                };
                show_lookup_origin(interaction, refreshed.origin)?;
                show_warnings(interaction, &refreshed.warnings)?;
                let mut refreshed_warning_state = warning_state.clone();
                refreshed_warning_state.set_metadata(refreshed.warnings);
                let identified = identify(
                    interaction,
                    source,
                    &inspection,
                    &search,
                    refreshed.candidates,
                    &mut metadata_resolver,
                    &cache,
                    identification_adapters.as_mut(),
                    false,
                    true,
                )?;
                refreshed_warning_state.set_identification(identified.warnings);
                let refreshed_candidates = identified.decision.candidates().to_vec();
                let refreshed_selection = choose(interaction, &inspection, identified.decision)?;
                let metadata_replaced = if refreshed_selection == MetadataSelection::Cancelled {
                    interaction.prose("Current preview kept unchanged.")?;
                    false
                } else if same_result(&metadata, &refreshed_selection) {
                    interaction.success("Provider data is current; the preview did not change.")?;
                    true
                } else if confirm_refreshed(interaction)? {
                    true
                } else {
                    interaction.prose("Current preview kept unchanged.")?;
                    false
                };
                if metadata_replaced {
                    metadata = refreshed_selection;
                    match_selection = match metadata {
                        MetadataSelection::Provider(_) => MatchSelection::UserChosen,
                        MetadataSelection::ExistingTags => MatchSelection::ExistingTags,
                        MetadataSelection::Cancelled => match_selection,
                    };
                    candidates = refreshed_candidates;
                    if identified.evidence_replaced {
                        identification = identified.evidence;
                    }
                    let (fallback, selection_warnings) =
                        selection_year_warnings(&inspection, &mut metadata);
                    source_year_fallback = fallback;
                    refreshed_warning_state.set_selection(selection_warnings);
                    warning_state = refreshed_warning_state;
                }
                let refreshed_artwork = fetch_artwork(
                    interaction,
                    &mut artwork_resolver,
                    &inspection,
                    &metadata,
                    false,
                    true,
                )?;
                if archive_artwork != refreshed_artwork.artwork {
                    show_artwork_change(
                        interaction,
                        archive_artwork.as_ref(),
                        refreshed_artwork.artwork.as_ref(),
                    )?;
                    if confirm_refreshed_artwork(interaction)? {
                        archive_artwork = refreshed_artwork.artwork;
                        if matches!(artwork, ArtworkSelection::CoverArtArchive(_))
                            || source.selected_artwork.is_none()
                        {
                            artwork = initial_artwork(source, &metadata, archive_artwork.as_ref());
                        }
                        set_artwork_warnings(
                            interaction,
                            &mut warning_state,
                            refreshed_artwork.warnings,
                            &artwork,
                            archive_artwork.as_ref(),
                        )?;
                    } else {
                        interaction.prose("Current artwork choice kept unchanged.")?;
                    }
                } else {
                    interaction.success("Cover Art Archive artwork is current.")?;
                    set_artwork_warnings(
                        interaction,
                        &mut warning_state,
                        refreshed_artwork.warnings,
                        &artwork,
                        archive_artwork.as_ref(),
                    )?;
                }
            }
            "" => {}
            "d" | "done" | "q" | "quit" => break,
            _ => interaction.error("Please choose Review, Artwork, Refresh, or Done.")?,
        }
    }

    let selected_has_fingerprint = matches!(
        &metadata,
        MetadataSelection::Provider(selected)
            if identification
                .as_ref()
                .is_some_and(|evidence| evidence.supports_candidate(selected))
    );
    let metadata_provenance = match &metadata {
        MetadataSelection::Provider(_) if selected_has_fingerprint => source_year_fallback.map_or(
            MetadataProvenance::MusicBrainzWithFingerprint,
            MetadataProvenance::MusicBrainzWithFingerprintAndSourceYear,
        ),
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
        archive_artwork,
        identification,
        warnings: warning_state.current(),
        match_selection,
    })
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
    interaction.section_heading("Metadata preview")?;
    match metadata {
        MetadataSelection::Provider(selected) => {
            interaction.field("Selected", selected.candidate.human_label())?;
            interaction.field("Verification", "MusicBrainz")?;
            if let Some(year) = source_year_fallback {
                interaction.field(
                    "Year provenance",
                    format!("source tags ({year}, unverified fallback)"),
                )?;
            } else {
                interaction.field("Year provenance", "MusicBrainz")?;
            }
        }
        MetadataSelection::ExistingTags => {
            interaction.field("Selected", "existing source metadata")?;
            interaction.field("Verification", "unverified")?;
            if common_release_group_id(&source_inspection(source).0).is_some() {
                interaction.field(
                    "Artwork provenance",
                    "Cover Art Archive via existing source ID; metadata remains unverified",
                )?;
            }
        }
        MetadataSelection::Cancelled => {}
    }
    interaction.field("Artwork", artwork_label(source, artwork))?;
    if let Some(archive) = archive
        && !matches!(artwork, ArtworkSelection::CoverArtArchive(_))
    {
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "  ")
                .with(SemanticRole::FieldName, "Artwork alternative")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Alternative,
                    format!(
                        "Cover Art Archive front ({} {}x{})",
                        archive.format, archive.dimensions.0, archive.dimensions.1
                    ),
                ),
        )?;
    }
    if warnings.is_empty() {
        interaction.field("Warnings", "none")?;
    } else {
        interaction.field("Warnings", format!("{} (shown above)", warnings.len()))?;
    }
    interaction.prose("  No files were changed. Choose Done to continue to the exact plan.")
}

pub fn revise_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    result: &mut GuidedMatchResult,
    viewer: &mut V,
) -> io::Result<()> {
    result.artwork = choose_artwork(
        interaction,
        source,
        result.archive_artwork.as_ref(),
        result.artwork.clone(),
        viewer,
    )?;
    Ok(())
}

fn review(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    inspection: &crate::domain::Inspection,
    candidates: &[RankedCandidate],
    metadata: &mut MetadataSelection,
    warnings: &[String],
    identification: Option<&FingerprintEvidence>,
) -> io::Result<bool> {
    loop {
        let identification_action = if identification.is_some() {
            "  [i] Identification"
        } else {
            ""
        };
        let answer = interaction.prompt(UiLine::menu_prompt(format!(
            "Review: [s] Source files and tags  [m] Metadata{identification_action}  [w] Warnings  [b] Back: "
        )))?;
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
                    interaction.prose("No warnings.")?;
                } else {
                    show_warnings(interaction, warnings)?;
                }
            }
            "i" | "identification" if identification.is_some() => {
                show_identification_details(interaction, identification.expect("checked above"))?;
            }
            "" | "b" | "back" => return Ok(false),
            _ => interaction
                .error("Please choose Source, Metadata, Identification, Warnings, or Back.")?,
        }
    }
}

fn show_source_review(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
) -> io::Result<()> {
    interaction.section_heading("Source files and tags")?;
    for audio in &source.audio {
        interaction.present(UiLine::new().with(
            SemanticRole::Path,
            format!("  {}", audio.relative_path.display()),
        ))?;
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "    ")
                .with(SemanticRole::FieldName, "title")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Value,
                    audio.tags.title.as_deref().unwrap_or("?"),
                )
                .with(SemanticRole::Prose, " | ")
                .with(SemanticRole::FieldName, "artist")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Value,
                    audio.tags.artist.as_deref().unwrap_or("?"),
                )
                .with(SemanticRole::Prose, " | ")
                .with(SemanticRole::FieldName, "album")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Value,
                    audio.tags.album.as_deref().unwrap_or("?"),
                )
                .with(SemanticRole::Prose, " | ")
                .with(SemanticRole::FieldName, "album artist")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Value,
                    audio.tags.album_artist.as_deref().unwrap_or("?"),
                )
                .with(SemanticRole::Prose, " | ")
                .with(SemanticRole::FieldName, "disc-track")
                .with(SemanticRole::Prose, ": ")
                .with(
                    SemanticRole::Value,
                    format!(
                        "{}-{}",
                        audio
                            .tags
                            .disc
                            .map_or_else(|| "?".into(), |value| value.to_string()),
                        audio
                            .tags
                            .track
                            .map_or_else(|| "?".into(), |value| value.to_string()),
                    ),
                ),
        )?;
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

fn common_release_group_id(inspection: &crate::domain::Inspection) -> Option<&str> {
    let first = inspection.tracks.first()?.release_group_id.as_deref()?;
    inspection
        .tracks
        .iter()
        .all(|track| track.release_group_id.as_deref() == Some(first))
        .then_some(first)
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
        let answer = interaction.prompt(UiLine::menu_prompt(
            "Use the materially changed refreshed metadata? [y/N]: ",
        ))?;
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer Yes or No.")?,
        }
    }
}

fn show_lookup_origin(interaction: &mut impl Interaction, origin: LookupOrigin) -> io::Result<()> {
    let message = match origin {
        LookupOrigin::Live => "MusicBrainz lookup completed.",
        LookupOrigin::Refreshed => "MusicBrainz data refreshed.",
        LookupOrigin::FreshCache => "Fresh metadata cache hit; MusicBrainz was not contacted.",
        LookupOrigin::FreshFallback => "Using cached metadata after refresh failure.",
        LookupOrigin::StaleFallback => "Using stale cached metadata after refresh failure.",
        LookupOrigin::OfflineStaleCache => "Using stale cached metadata in offline mode.",
        LookupOrigin::OfflineMiss => "No cached metadata is available in offline mode.",
        LookupOrigin::ProviderUnavailable => "MusicBrainz metadata is unavailable.",
        LookupOrigin::InsufficientEvidence => {
            "MusicBrainz lookup skipped; the source has too little metadata."
        }
    };
    interaction.success(message)
}

fn show_acoustid_origin(
    interaction: &mut impl Interaction,
    origin: AcoustIdLookupOrigin,
) -> io::Result<()> {
    let message = match origin {
        AcoustIdLookupOrigin::Live => "AcoustID lookup completed.",
        AcoustIdLookupOrigin::Refreshed => "AcoustID identification refreshed.",
        AcoustIdLookupOrigin::FreshCache => {
            "Fresh identification cache hit; AcoustID was not contacted."
        }
        AcoustIdLookupOrigin::FreshFallback => {
            "Using cached identification after AcoustID refresh failure."
        }
        AcoustIdLookupOrigin::StaleFallback => {
            "Using stale cached identification after AcoustID refresh failure."
        }
        AcoustIdLookupOrigin::OfflineStaleCache => {
            "Using stale cached identification in offline mode."
        }
        AcoustIdLookupOrigin::OfflineMiss => {
            "No cached audio identification is available in offline mode."
        }
        AcoustIdLookupOrigin::ProviderUnavailable => "AcoustID identification is unavailable.",
    };
    interaction.success(message)
}

fn show_identification_summary(
    interaction: &mut impl Interaction,
    evidence: &FingerprintEvidence,
) -> io::Result<()> {
    if evidence.recordings.is_empty() {
        if evidence.recognized_without_recording {
            interaction.warning(
                "AcoustID recognized the audio but has no MusicBrainz recording association.",
            )
        } else {
            interaction.warning("Audio fingerprint found no usable recording match.")
        }
    } else if evidence.recordings.len() == 1 {
        interaction.success("Audio fingerprint supports this recording.")
    } else {
        interaction.warning("Audio fingerprint was ambiguous; the candidate releases need review.")
    }
}

fn show_identification_details(
    interaction: &mut impl Interaction,
    evidence: &FingerprintEvidence,
) -> io::Result<()> {
    interaction.section_heading("Audio identification evidence")?;
    interaction.field("Provider", "AcoustID lookup; fingerprint and duration only")?;
    if evidence.recordings.is_empty() {
        return interaction.field("MusicBrainz recordings", "none");
    }
    for recording in &evidence.recordings {
        interaction.field("MusicBrainz recording", &recording.recording_id)?;
        for association in &recording.associations {
            interaction.prose(format!(
                "    AcoustID {} — raw score {:.3}",
                association.result_id, association.score
            ))?;
        }
    }
    Ok(())
}

fn show_warnings(interaction: &mut impl Interaction, warnings: &[String]) -> io::Result<()> {
    for warning in warnings {
        interaction.warning(format!("Warning: {warning}"))?;
    }
    Ok(())
}

struct InteractionProgress<'a, I>(&'a mut I);

impl<I: Interaction> ProviderProgress for InteractionProgress<'_, I> {
    fn event(&mut self, event: ProviderEvent) -> Result<(), ProviderError> {
        let message = match event {
            ProviderEvent::Requesting {
                provider: _,
                operation,
            } => format!("  {operation}..."),
            ProviderEvent::Waiting {
                provider,
                seconds,
                reason: WaitReason::RateLimit,
            } => format!("  Waiting {seconds}s for {provider}'s rate limit..."),
            ProviderEvent::Waiting {
                provider,
                seconds,
                reason: WaitReason::Retry,
            } => format!("  {provider} unavailable; retrying in {seconds}s (Ctrl-C exits)..."),
            ProviderEvent::RetryingTitle {
                original,
                simplified,
            } => format!(
                "  No usable release found for “{original}”; retrying with “{simplified}”..."
            ),
        };
        self.0
            .status(UiLine::prose(message))
            .map_err(|error| ProviderError::Progress(error.to_string()))
    }
}

impl<I: Interaction> FingerprintProgress for InteractionProgress<'_, I> {
    fn calculating(&mut self, audio: &std::path::Path) -> Result<(), FingerprintError> {
        self.0
            .present(
                UiLine::new()
                    .with(
                        SemanticRole::Prose,
                        "  Calculating local audio fingerprint for ",
                    )
                    .with(SemanticRole::Path, audio.display().to_string())
                    .with(SemanticRole::Prose, " (up to 120 seconds of audio)..."),
            )
            .map_err(|error| FingerprintError::Progress(error.to_string()))
    }
}

#[cfg(test)]
mod tests;
