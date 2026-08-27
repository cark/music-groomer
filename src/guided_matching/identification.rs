use std::io;
use std::time::SystemTime;

use crate::domain::{CandidateRelease, Inspection, SourceKind};
use crate::fingerprint::AudioFingerprinter;
use crate::identification::{FingerprintEvidence, needs_fingerprint};
use crate::matching::{MatchDecision, MatchPolicy};
use crate::provider::{
    AcoustIdProvider, AcoustIdResolver, MetadataProvider, MetadataResolver, ProviderCache,
    ProviderSearch, collapse_equivalent,
};
use crate::source::SourceInspection;
use crate::terminal::Interaction;

use super::{
    InteractionProgress, show_acoustid_origin, show_identification_summary, show_lookup_origin,
    show_warnings,
};

pub(super) struct IdentificationAdapters<'a> {
    pub(super) fingerprinter: &'a mut dyn AudioFingerprinter,
    pub(super) acoustid_provider: &'a mut dyn AcoustIdProvider,
}

pub(super) struct IdentificationOutcome {
    pub(super) decision: MatchDecision,
    pub(super) evidence: Option<FingerprintEvidence>,
    pub(super) evidence_replaced: bool,
    pub(super) warnings: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn identify<M: MetadataProvider>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    inspection: &Inspection,
    search: &ProviderSearch,
    base_candidates: Vec<CandidateRelease>,
    metadata_resolver: &mut MetadataResolver<M>,
    cache: &ProviderCache,
    adapters: Option<&mut IdentificationAdapters<'_>>,
    offline: bool,
    force_refresh: bool,
) -> io::Result<IdentificationOutcome> {
    let base_decision = MatchPolicy::default().decide(inspection, base_candidates.clone());
    if !needs_fingerprint(source, inspection, &base_decision) {
        return Ok(IdentificationOutcome {
            decision: base_decision,
            evidence: None,
            evidence_replaced: true,
            warnings: Vec::new(),
        });
    }
    let Some(adapters) = adapters else {
        return Ok(IdentificationOutcome {
            decision: base_decision,
            evidence: None,
            evidence_replaced: false,
            warnings: Vec::new(),
        });
    };

    let fingerprint = {
        let mut progress = InteractionProgress(interaction);
        adapters
            .fingerprinter
            .calculate(&selected_audio_path(source), &mut progress)
    };
    let fingerprint = match fingerprint {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            let warning = if force_refresh {
                format!(
                    "Audio fingerprint refresh failed ({error}); keeping the current identification"
                )
            } else {
                format!(
                    "Audio fingerprint identification is unavailable ({error}); keeping provider and source metadata"
                )
            };
            interaction.warning(format!("Warning: {warning}"))?;
            return Ok(IdentificationOutcome {
                decision: base_decision,
                evidence: None,
                evidence_replaced: false,
                warnings: vec![warning],
            });
        }
    };

    interaction
        .prose("  AcoustID receives this compact fingerprint and duration, not the audio file.")?;
    let lookup = {
        let mut resolver = AcoustIdResolver::new(&mut *adapters.acoustid_provider, cache.clone());
        let mut progress = InteractionProgress(interaction);
        resolver.lookup(
            &fingerprint,
            offline,
            force_refresh,
            SystemTime::now(),
            &mut progress,
        )
    };
    show_acoustid_origin(interaction, lookup.origin)?;
    show_warnings(interaction, &lookup.warnings)?;
    let mut warnings = lookup.warnings;
    let Some(response) = lookup.response else {
        return Ok(IdentificationOutcome {
            decision: base_decision,
            evidence: None,
            evidence_replaced: false,
            warnings,
        });
    };

    let evidence =
        FingerprintEvidence::from_response(response, inspection, fingerprint.duration_seconds);
    show_identification_summary(interaction, &evidence)?;
    if evidence.unusually_ambiguous {
        warnings.push(
            "Audio fingerprint produced more than five qualifying MusicBrainz recordings; only the five strongest were resolved"
                .into(),
        );
    }
    let recording_ids = evidence.recording_ids();
    if recording_ids.is_empty() {
        return Ok(IdentificationOutcome {
            decision: base_decision,
            evidence: Some(evidence),
            evidence_replaced: true,
            warnings,
        });
    }

    let mut recording_search = search.clone();
    recording_search.kind = SourceKind::LooseFile;
    recording_search.release_group_id = None;
    recording_search.recording_ids = recording_ids.clone();
    let resolved = {
        let mut progress = InteractionProgress(interaction);
        metadata_resolver.lookup(
            &recording_search,
            offline,
            force_refresh,
            SystemTime::now(),
            &mut progress,
        )
    };
    show_lookup_origin(interaction, resolved.origin)?;
    show_warnings(interaction, &resolved.warnings)?;
    warnings.extend(resolved.warnings);

    let mut candidates = base_candidates;
    candidates.extend(resolved.candidates);
    let candidates = collapse_equivalent(candidates);
    let decision = MatchPolicy::default().decide_with_fingerprint(
        inspection,
        candidates,
        &recording_ids,
        evidence.automatic_recording_id.is_some(),
    );
    Ok(IdentificationOutcome {
        decision,
        evidence: Some(evidence),
        evidence_replaced: true,
        warnings,
    })
}

fn selected_audio_path(source: &SourceInspection) -> std::path::PathBuf {
    if source.kind == SourceKind::LooseFile {
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
