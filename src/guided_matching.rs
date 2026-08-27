use std::io;
use std::time::SystemTime;

use crate::artwork_viewer::ArtworkViewer;
use crate::fingerprint::{AudioFingerprinter, FingerprintError, FingerprintProgress};
use crate::identification::{FingerprintEvidence, needs_fingerprint};
use crate::matching::MatchPolicy;
use crate::matching::RankedCandidate;
use crate::matching_ui::{MetadataSelection, choose, revise};
use crate::provider::{
    AcoustIdLookupOrigin, AcoustIdProvider, AcoustIdResolver, ArtworkLookup, ArtworkProvider,
    ArtworkResolver, LookupOrigin, MetadataProvider, MetadataResolver, ProviderCache,
    ProviderError, ProviderEvent, ProviderProgress, WaitReason, collapse_equivalent,
    equivalent_groomed_result, source_inspection,
};
use crate::source::SourceInspection;
use crate::terminal::{Interaction, SemanticRole, UiLine};

pub struct GuidedMatchResult {
    pub metadata: MetadataSelection,
    pub metadata_provenance: MetadataProvenance,
    pub candidates: Vec<RankedCandidate>,
    pub artwork: ArtworkSelection,
    pub identification: Option<FingerprintEvidence>,
    pub warnings: Vec<String>,
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

struct IdentificationAdapters<'a> {
    fingerprinter: &'a mut dyn AudioFingerprinter,
    acoustid_provider: &'a mut dyn AcoustIdProvider,
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
    interaction.blank()?;
    if offline {
        interaction.heading("Metadata lookup (offline: providers will not be contacted)")?;
    } else {
        interaction.heading("Checking metadata and provider cache")?;
    }

    let mut metadata_resolver = MetadataResolver::new(metadata, cache.clone());
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
    let mut provider_candidates = lookup.candidates;
    let mut decision = MatchPolicy::default().decide(&inspection, provider_candidates.clone());
    let mut identification = None;
    if needs_fingerprint(source, &inspection, &decision)
        && let Some(adapters) = identification_adapters.as_mut()
    {
        let audio_path = selected_audio_path(source);
        let fingerprint = {
            let mut progress = InteractionProgress(interaction);
            adapters.fingerprinter.calculate(&audio_path, &mut progress)
        };
        match fingerprint {
            Ok(fingerprint) => {
                interaction.prose(
                    "  AcoustID receives this compact fingerprint and duration, not the audio file.",
                )?;
                let acoustid_lookup = {
                    let mut resolver =
                        AcoustIdResolver::new(&mut *adapters.acoustid_provider, cache.clone());
                    let mut progress = InteractionProgress(interaction);
                    resolver.lookup(
                        &fingerprint,
                        offline,
                        false,
                        SystemTime::now(),
                        &mut progress,
                    )
                };
                show_acoustid_origin(interaction, acoustid_lookup.origin)?;
                show_warnings(interaction, &acoustid_lookup.warnings)?;
                warnings.extend(acoustid_lookup.warnings);
                if let Some(response) = acoustid_lookup.response {
                    let evidence = FingerprintEvidence::from_response(
                        response,
                        &inspection,
                        fingerprint.duration_seconds,
                    );
                    show_identification_summary(interaction, &evidence)?;
                    if evidence.unusually_ambiguous {
                        warnings.push(
                            "Audio fingerprint produced more than five qualifying MusicBrainz recordings; only the five strongest were resolved"
                                .into(),
                        );
                    }
                    let recording_ids = evidence.recording_ids();
                    if !recording_ids.is_empty() {
                        let mut recording_search = search.clone();
                        recording_search.kind = crate::domain::SourceKind::LooseFile;
                        recording_search.release_group_id = None;
                        recording_search.recording_ids = recording_ids.clone();
                        let resolved = {
                            let mut progress = InteractionProgress(interaction);
                            metadata_resolver.lookup(
                                &recording_search,
                                offline,
                                false,
                                SystemTime::now(),
                                &mut progress,
                            )
                        };
                        show_lookup_origin(interaction, resolved.origin)?;
                        show_warnings(interaction, &resolved.warnings)?;
                        warnings.extend(resolved.warnings);
                        provider_candidates.extend(resolved.candidates);
                        provider_candidates = collapse_equivalent(provider_candidates);
                        decision = MatchPolicy::default().decide_with_fingerprint(
                            &inspection,
                            provider_candidates.clone(),
                            &recording_ids,
                            evidence.automatic_recording_id.is_some(),
                        );
                    }
                    identification = Some(evidence);
                }
            }
            Err(error) => {
                let warning = format!(
                    "Audio fingerprint identification is unavailable ({error}); keeping provider and source metadata"
                );
                interaction.warning(format!("Warning: {warning}"))?;
                warnings.push(warning);
            }
        }
        deduplicate(&mut warnings);
    }
    let mut candidates = decision.candidates().to_vec();
    let mut metadata = choose(interaction, &inspection, decision)?;
    if metadata == MetadataSelection::Cancelled {
        return Ok(GuidedMatchResult {
            metadata,
            metadata_provenance: MetadataProvenance::None,
            candidates,
            artwork: ArtworkSelection::None,
            identification,
            warnings,
        });
    }
    let mut source_year_fallback = add_year_fallback(&inspection, &mut metadata, &mut warnings);

    let mut artwork_resolver = ArtworkResolver::new(artwork, cache.clone());
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
                let refreshed_decision = if identification.is_some() {
                    if let Some(adapters) = identification_adapters.as_mut() {
                        refresh_fingerprint_identification(
                            interaction,
                            source,
                            &mut metadata_resolver,
                            &cache,
                            adapters,
                            &mut warnings,
                        )?
                        .and_then(|refreshed| {
                            identification = Some(refreshed.evidence);
                            refreshed.decision
                        })
                    } else {
                        None
                    }
                } else {
                    let refreshed = {
                        let mut progress = InteractionProgress(interaction);
                        metadata_resolver.lookup(
                            &search,
                            false,
                            true,
                            SystemTime::now(),
                            &mut progress,
                        )
                    };
                    show_lookup_origin(interaction, refreshed.origin)?;
                    show_warnings(interaction, &refreshed.warnings)?;
                    warnings.extend(refreshed.warnings);
                    Some(MatchPolicy::default().decide(&inspection, refreshed.candidates))
                };
                deduplicate(&mut warnings);
                let metadata_replaced = if let Some(refreshed_decision) = refreshed_decision {
                    candidates = refreshed_decision.candidates().to_vec();
                    let refreshed_selection = choose(interaction, &inspection, refreshed_decision)?;
                    if refreshed_selection == MetadataSelection::Cancelled {
                        interaction.prose("Current preview kept unchanged.")?;
                        false
                    } else if same_result(&metadata, &refreshed_selection) {
                        interaction
                            .success("Provider data is current; the preview did not change.")?;
                        metadata = refreshed_selection;
                        true
                    } else if confirm_refreshed(interaction)? {
                        metadata = refreshed_selection;
                        true
                    } else {
                        interaction.prose("Current preview kept unchanged.")?;
                        false
                    }
                } else {
                    interaction.prose("Current preview kept unchanged.")?;
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
                        interaction.prose("Current artwork choice kept unchanged.")?;
                    }
                } else {
                    interaction.success("Cover Art Archive artwork is current.")?;
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
        identification,
        warnings,
    })
}

struct RefreshedIdentification {
    evidence: FingerprintEvidence,
    decision: Option<crate::matching::MatchDecision>,
}

fn refresh_fingerprint_identification<M: MetadataProvider>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    metadata_resolver: &mut MetadataResolver<M>,
    cache: &ProviderCache,
    adapters: &mut IdentificationAdapters<'_>,
    warnings: &mut Vec<String>,
) -> io::Result<Option<RefreshedIdentification>> {
    let (inspection, search) = source_inspection(source);
    let fingerprint = {
        let mut progress = InteractionProgress(interaction);
        adapters
            .fingerprinter
            .calculate(&selected_audio_path(source), &mut progress)
    };
    let fingerprint = match fingerprint {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            let warning = format!(
                "Audio fingerprint refresh failed ({error}); keeping the current identification"
            );
            interaction.warning(format!("Warning: {warning}"))?;
            warnings.push(warning);
            return Ok(None);
        }
    };
    interaction
        .prose("  AcoustID receives this compact fingerprint and duration, not the audio file.")?;
    let lookup = {
        let mut resolver = AcoustIdResolver::new(&mut *adapters.acoustid_provider, cache.clone());
        let mut progress = InteractionProgress(interaction);
        resolver.lookup(&fingerprint, false, true, SystemTime::now(), &mut progress)
    };
    show_acoustid_origin(interaction, lookup.origin)?;
    show_warnings(interaction, &lookup.warnings)?;
    warnings.extend(lookup.warnings);
    let Some(response) = lookup.response else {
        return Ok(None);
    };
    let evidence =
        FingerprintEvidence::from_response(response, &inspection, fingerprint.duration_seconds);
    show_identification_summary(interaction, &evidence)?;
    let recording_ids = evidence.recording_ids();
    if recording_ids.is_empty() {
        return Ok(Some(RefreshedIdentification {
            evidence,
            decision: None,
        }));
    }
    let mut recording_search = search;
    recording_search.kind = crate::domain::SourceKind::LooseFile;
    recording_search.release_group_id = None;
    recording_search.recording_ids = recording_ids.clone();
    let resolved = {
        let mut progress = InteractionProgress(interaction);
        metadata_resolver.lookup(
            &recording_search,
            false,
            true,
            SystemTime::now(),
            &mut progress,
        )
    };
    show_lookup_origin(interaction, resolved.origin)?;
    show_warnings(interaction, &resolved.warnings)?;
    warnings.extend(resolved.warnings);
    let candidates = collapse_equivalent(resolved.candidates);
    let decision = MatchPolicy::default().decide_with_fingerprint(
        &inspection,
        candidates,
        &recording_ids,
        evidence.automatic_recording_id.is_some(),
    );
    Ok(Some(RefreshedIdentification {
        evidence,
        decision: Some(decision),
    }))
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
    interaction.prose("Checking Cover Art Archive for a canonical front cover...")?;
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
    interaction.blank()?;
    interaction.heading("metadata preview")?;
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
    if archive.is_some() && !matches!(artwork, ArtworkSelection::CoverArtArchive(_)) {
        interaction.present(
            UiLine::new()
                .with(SemanticRole::Prose, "  ")
                .with(SemanticRole::FieldName, "Artwork alternative")
                .with(SemanticRole::Prose, ": ")
                .with(SemanticRole::Alternative, "Cover Art Archive 1200px front"),
        )?;
    }
    if warnings.is_empty() {
        interaction.field("Warnings", "none")?;
    } else {
        interaction.field("Warnings", format!("{} (shown above)", warnings.len()))?;
    }
    interaction.prose("  No files were changed. Apply arrives in milestone 4.")
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
        Ok(()) => interaction.success("Opened the selected artwork in the system image viewer."),
        Err(error) => interaction.error(format!("Could not view artwork: {error}")),
    }
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
    interaction.blank()?;
    interaction.heading("Source files and tags")?;
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
    interaction.heading("Cover Art Archive artwork changed")?;
    interaction.field("Previous", provider_artwork_label(previous))?;
    interaction.field("Refreshed", provider_artwork_label(refreshed))
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
    interaction.blank()?;
    interaction.heading("Audio identification evidence")?;
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

fn selected_audio_path(source: &SourceInspection) -> std::path::PathBuf {
    if source.kind == crate::domain::SourceKind::LooseFile {
        source.source.clone()
    } else {
        source.source.join(
            &source
                .audio
                .first()
                .expect("identification requires one inspected audio file")
                .relative_path,
        )
    }
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
