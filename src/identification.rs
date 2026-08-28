use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::domain::Inspection;
use crate::matching::{MatchDecision, RankedCandidate};
use crate::provider::{ACOUSTID_USABLE_SCORE, AcoustIdResponse, AcoustIdResult};
use crate::source::SourceInspection;

pub const ACOUSTID_CANDIDATE_SCORE: f64 = ACOUSTID_USABLE_SCORE;
pub const ACOUSTID_AUTO_SCORE: f64 = 0.90;
pub const MAX_RECORDING_CANDIDATES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct FingerprintEvidence {
    pub provider_results: Vec<AcoustIdResult>,
    pub recordings: Vec<RecordingEvidence>,
    pub unusually_ambiguous: bool,
    pub recognized_without_recording: bool,
    pub automatic_recording_id: Option<String>,
}

impl FingerprintEvidence {
    pub fn from_response(
        response: AcoustIdResponse,
        inspection: &Inspection,
        fingerprint_duration_seconds: u32,
    ) -> Self {
        let mut grouped = BTreeMap::<String, Vec<AcoustIdAssociation>>::new();
        for result in &response.results {
            if result.score < ACOUSTID_CANDIDATE_SCORE {
                continue;
            }
            let mut seen_recordings = std::collections::BTreeSet::new();
            for recording_id in &result.recording_ids {
                if !seen_recordings.insert(recording_id) {
                    continue;
                }
                grouped
                    .entry(recording_id.clone())
                    .or_default()
                    .push(AcoustIdAssociation {
                        result_id: result.id.clone(),
                        score: result.score,
                    });
            }
        }
        let unusually_ambiguous = grouped.len() > MAX_RECORDING_CANDIDATES;
        let mut recordings = grouped
            .into_iter()
            .map(|(recording_id, associations)| RecordingEvidence {
                recording_id,
                associations,
            })
            .collect::<Vec<_>>();
        recordings.sort_by(|left, right| {
            right
                .best_score()
                .partial_cmp(&left.best_score())
                .unwrap_or(Ordering::Equal)
                .then_with(|| right.associations.len().cmp(&left.associations.len()))
                .then_with(|| left.recording_id.cmp(&right.recording_id))
        });
        recordings.truncate(MAX_RECORDING_CANDIDATES);

        let source = inspection.tracks.first();
        let all_qualifying_results_have_one_recording = recordings.first().is_some_and(|sole| {
            recordings.len() == 1
                && response
                    .results
                    .iter()
                    .filter(|result| result.score >= ACOUSTID_CANDIDATE_SCORE)
                    .all(|result| {
                        !result.recording_ids.is_empty()
                            && result
                                .recording_ids
                                .iter()
                                .all(|id| id == &sole.recording_id)
                    })
        });
        let automatic_recording_id = (recordings.len() == 1
            && recordings[0].best_score() >= ACOUSTID_AUTO_SCORE
            && all_qualifying_results_have_one_recording
            && source.is_some_and(|track| {
                duration_compatible(
                    track.duration_ms,
                    u64::from(fingerprint_duration_seconds) * 1_000,
                )
            })
            && source
                .and_then(|track| track.recording_id.as_ref())
                .is_none_or(|existing| existing == &recordings[0].recording_id))
        .then(|| recordings[0].recording_id.clone());
        let recognized_without_recording = !response.results.is_empty()
            && !response
                .results
                .iter()
                .any(|result| !result.recording_ids.is_empty());

        Self {
            provider_results: response.results,
            recordings,
            unusually_ambiguous,
            recognized_without_recording,
            automatic_recording_id,
        }
    }

    pub fn recording_ids(&self) -> Vec<String> {
        self.recordings
            .iter()
            .map(|recording| recording.recording_id.clone())
            .collect()
    }

    pub fn supports_candidate(&self, candidate: &RankedCandidate) -> bool {
        candidate.mappings.iter().any(|mapping| {
            candidate
                .candidate
                .tracks
                .get(mapping.candidate_index)
                .and_then(|track| track.recording_id.as_ref())
                .is_some_and(|id| self.recordings.iter().any(|item| &item.recording_id == id))
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordingEvidence {
    pub recording_id: String,
    pub associations: Vec<AcoustIdAssociation>,
}

impl RecordingEvidence {
    pub fn best_score(&self) -> f64 {
        self.associations
            .iter()
            .map(|association| association.score)
            .fold(0.0, f64::max)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcoustIdAssociation {
    pub result_id: String,
    pub score: f64,
}

pub fn needs_fingerprint(
    source: &SourceInspection,
    inspection: &Inspection,
    decision: &MatchDecision,
) -> bool {
    if source.audio.len() != 1 {
        return false;
    }
    let Some(track) = inspection.tracks.first() else {
        return false;
    };
    if track.recording_id.is_some() || track.release_group_id.is_some() {
        return false;
    }
    if credible_release_tags(track) {
        return false;
    }
    match decision {
        MatchDecision::Selected { .. } => false,
        MatchDecision::NeedsChoice(candidates) => distinct_mapped_recordings(candidates) != 1,
        MatchDecision::NoUsableMatch(_) => true,
    }
}

fn credible_release_tags(track: &crate::domain::InspectedTrack) -> bool {
    track
        .album
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
        && track
            .album_artist
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        && track.position.is_some()
}

fn distinct_mapped_recordings(candidates: &[RankedCandidate]) -> usize {
    candidates
        .iter()
        .flat_map(|candidate| {
            candidate.mappings.iter().filter_map(|mapping| {
                candidate
                    .candidate
                    .tracks
                    .get(mapping.candidate_index)
                    .and_then(|track| track.recording_id.as_deref())
            })
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn duration_compatible(source_ms: u64, fingerprint_ms: u64) -> bool {
    source_ms == 0 || fingerprint_ms == 0 || source_ms.abs_diff(fingerprint_ms) <= 7_000
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::domain::{InspectedTrack, SourceKind};
    use crate::source::{AudioFormat, AudioProperties, AudioTags, InspectedAudio};

    #[test]
    fn score_gates_deduplicate_and_cap_recordings() {
        let response = AcoustIdResponse {
            results: (0..8)
                .map(|index| AcoustIdResult {
                    id: format!("result-{index}"),
                    score: if index == 7 {
                        0.79
                    } else {
                        0.80 + index as f64 / 100.0
                    },
                    recording_ids: vec![format!("recording-{index}")],
                })
                .collect(),
        };

        let evidence = FingerprintEvidence::from_response(response, &inspection(), 180);

        assert_eq!(evidence.recordings.len(), MAX_RECORDING_CANDIDATES);
        assert!(evidence.unusually_ambiguous);
        assert_eq!(evidence.recordings[0].recording_id, "recording-6");
        assert!(!evidence.recording_ids().contains(&"recording-7".into()));
    }

    #[test]
    fn equal_scores_favor_recordings_corroborated_by_more_results() {
        let response = AcoustIdResponse {
            results: vec![
                AcoustIdResult {
                    id: "strong-result".into(),
                    score: 0.97,
                    recording_ids: vec![
                        "album-a".into(),
                        "album-b".into(),
                        "album-c".into(),
                        "album-d".into(),
                        "album-e".into(),
                        "single-recording".into(),
                    ],
                },
                AcoustIdResult {
                    id: "supporting-result-a".into(),
                    score: 0.95,
                    recording_ids: vec!["single-recording".into()],
                },
                AcoustIdResult {
                    id: "supporting-result-b".into(),
                    score: 0.93,
                    recording_ids: vec!["single-recording".into()],
                },
            ],
        };

        let evidence = FingerprintEvidence::from_response(response, &inspection(), 180);

        assert_eq!(evidence.recordings.len(), MAX_RECORDING_CANDIDATES);
        assert_eq!(evidence.recordings[0].recording_id, "single-recording");
        assert_eq!(evidence.recordings[0].associations.len(), 3);
    }

    #[test]
    fn one_strong_non_conflicting_recording_can_be_automatic() {
        let response = AcoustIdResponse {
            results: vec![AcoustIdResult {
                id: "result".into(),
                score: 0.92,
                recording_ids: vec!["recording".into(), "recording".into()],
            }],
        };

        let evidence = FingerprintEvidence::from_response(response, &inspection(), 180);

        assert_eq!(evidence.recordings.len(), 1);
        assert_eq!(
            evidence.automatic_recording_id.as_deref(),
            Some("recording")
        );
    }

    #[test]
    fn coherent_release_identity_does_not_trigger_fingerprinting() {
        let mut inspection = inspection();
        let track = &mut inspection.tracks[0];
        track.album = Some("Known Single".into());
        track.album_artist = Some("Known Artist".into());
        track.position = Some(crate::domain::Position::new(1, 1));

        assert!(!needs_fingerprint(
            &source(1),
            &inspection,
            &MatchDecision::NoUsableMatch(Vec::new())
        ));
    }

    #[test]
    fn album_selection_never_triggers_routine_fingerprinting() {
        let mut inspection = inspection();
        inspection.tracks.push(inspection.tracks[0].clone());

        assert!(!needs_fingerprint(
            &source(2),
            &inspection,
            &MatchDecision::NoUsableMatch(Vec::new())
        ));
    }

    fn inspection() -> Inspection {
        Inspection {
            source_label: "track.flac".into(),
            kind: SourceKind::LooseFile,
            tracks: vec![InspectedTrack {
                source_name: "track.flac".into(),
                title: None,
                artist: None,
                album: None,
                album_artist: None,
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: None,
                original_year: None,
                position: None,
                duration_ms: 180_000,
                recording_id: None,
                release_group_id: None,
            }],
        }
    }

    fn source(track_count: usize) -> SourceInspection {
        SourceInspection {
            source: PathBuf::from("incoming"),
            kind: if track_count == 1 {
                SourceKind::LooseFile
            } else {
                SourceKind::AlbumDirectory
            },
            audio: (0..track_count)
                .map(|index| InspectedAudio {
                    relative_path: PathBuf::from(format!("track-{index}.flac")),
                    format: AudioFormat::Flac,
                    properties: AudioProperties {
                        duration: Duration::from_secs(180),
                        sample_rate: Some(44_100),
                        channels: Some(2),
                        bit_depth: Some(16),
                        audio_bitrate: None,
                    },
                    tags: AudioTags::default(),
                })
                .collect(),
            ancillary: Vec::new(),
            artwork: Vec::new(),
            selected_artwork: None,
            notices: Vec::new(),
            snapshot: Vec::new(),
        }
    }
}
