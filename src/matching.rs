use std::cmp::Reverse;
use std::collections::BTreeSet;

use crate::domain::{CandidateRelease, InspectedTrack, Inspection, ReleaseTrack};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchReason {
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackMapping {
    pub source_index: usize,
    pub candidate_index: usize,
    pub basis: TrackMatchBasis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackMatchBasis {
    RecordingId,
    TitleAndDuration,
    PositionFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankedCandidate {
    pub candidate: CandidateRelease,
    pub mappings: Vec<TrackMapping>,
    pub reasons: Vec<MatchReason>,
    score: i32,
    complete: bool,
    credible_identity: bool,
    meaningful_track_evidence: bool,
    identifier_conflict: bool,
}

impl RankedCandidate {
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn is_credible(&self) -> bool {
        self.complete
            && self.credible_identity
            && self.meaningful_track_evidence
            && !self.identifier_conflict
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchDecision {
    Selected {
        selected: Box<RankedCandidate>,
        candidates: Vec<RankedCandidate>,
    },
    NeedsChoice(Vec<RankedCandidate>),
    NoUsableMatch(Vec<RankedCandidate>),
}

impl MatchDecision {
    pub fn candidates(&self) -> &[RankedCandidate] {
        match self {
            Self::Selected { candidates, .. }
            | Self::NeedsChoice(candidates)
            | Self::NoUsableMatch(candidates) => candidates,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchPolicy {
    minimum_auto_score: i32,
    minimum_lead: i32,
}

impl Default for MatchPolicy {
    fn default() -> Self {
        Self {
            minimum_auto_score: 300,
            minimum_lead: 60,
        }
    }
}

impl MatchPolicy {
    pub fn decide(
        &self,
        inspection: &Inspection,
        candidates: Vec<CandidateRelease>,
    ) -> MatchDecision {
        let mut ranked: Vec<_> = candidates
            .into_iter()
            .map(|candidate| rank(inspection, candidate))
            .collect();
        ranked.sort_by_key(|candidate| Reverse(candidate.score));

        if ranked.is_empty() {
            return MatchDecision::NoUsableMatch(Vec::new());
        }
        let mut usable: Vec<_> = ranked
            .iter()
            .filter(|candidate| candidate.complete)
            .cloned()
            .collect();
        if usable.is_empty() {
            return MatchDecision::NoUsableMatch(ranked);
        }

        if inspection.kind == crate::domain::SourceKind::LooseFile {
            let credible_single = usable.iter().any(|candidate| {
                candidate.is_credible()
                    && candidate.candidate.kind == crate::domain::ReleaseKind::Single
            });
            let credible_non_single = usable.iter().any(|candidate| {
                candidate.is_credible()
                    && candidate.candidate.kind != crate::domain::ReleaseKind::Single
            });
            if credible_single {
                usable.sort_by_key(|candidate| {
                    Reverse((
                        candidate.candidate.kind == crate::domain::ReleaseKind::Single,
                        candidate.score,
                    ))
                });
                if credible_non_single {
                    return MatchDecision::NeedsChoice(usable);
                }
            }
        }

        let best = &usable[0];
        let runner_up_score = usable.get(1).map_or(i32::MIN, |candidate| candidate.score);
        let clear_lead = best.score.saturating_sub(runner_up_score) >= self.minimum_lead;
        let accepted_kind = !matches!(best.candidate.kind, crate::domain::ReleaseKind::Other(_));
        let minimum_auto_score = if inspection.kind == crate::domain::SourceKind::LooseFile {
            120
        } else {
            self.minimum_auto_score
        };
        if best.score >= minimum_auto_score && clear_lead && best.is_credible() && accepted_kind {
            MatchDecision::Selected {
                selected: Box::new(best.clone()),
                candidates: usable,
            }
        } else {
            MatchDecision::NeedsChoice(usable)
        }
    }
}

fn rank(inspection: &Inspection, candidate: CandidateRelease) -> RankedCandidate {
    let mut score = 0;
    let mut reasons = Vec::new();
    let mut used = BTreeSet::new();
    let mut mappings = Vec::new();

    if inspection.kind.requires_complete_release() {
        if inspection.tracks.len() == candidate.tracks.len() {
            score += 120;
            reasons.push(reason(format!(
                "track count agrees at {}",
                inspection.tracks.len()
            )));
        } else {
            score -= 300;
            reasons.push(reason(format!(
                "track count differs: source {}, candidate {}",
                inspection.tracks.len(),
                candidate.tracks.len()
            )));
        }
    }

    for (source_index, source) in inspection.tracks.iter().enumerate() {
        let available: Vec<_> = candidate
            .tracks
            .iter()
            .enumerate()
            .filter(|(index, _)| !used.contains(index))
            .collect();
        let Some((candidate_index, target, basis)) = best_track(source, &available) else {
            reasons.push(reason(format!(
                "{} has no unique candidate track",
                source.source_name
            )));
            continue;
        };

        let evidence = track_evidence(source, target);
        score += evidence.score;
        reasons.extend(evidence.reasons);
        used.insert(candidate_index);
        mappings.push(TrackMapping {
            source_index,
            candidate_index,
            basis,
        });
    }

    if let Some(album) = common_string(&inspection.tracks, |track| &track.album)
        && normalize(&album) == normalize(&candidate.title)
    {
        score += 60;
        reasons.push(reason("album title agrees"));
    }
    if let Some(artist) = common_string(&inspection.tracks, |track| &track.album_artist)
        && normalize(&artist) == normalize(&candidate.album_artist.display)
    {
        score += 40;
        reasons.push(reason("album artist agrees"));
    }
    let release_group_agrees = common_string(&inspection.tracks, |track| &track.release_group_id)
        .is_some_and(|group| candidate.release_group_id.as_ref() == Some(&group));
    let release_group_conflicts =
        common_string(&inspection.tracks, |track| &track.release_group_id).is_some_and(|group| {
            candidate
                .release_group_id
                .as_ref()
                .is_some_and(|candidate| candidate != &group)
        });
    if release_group_agrees {
        score += 500;
        reasons.push(reason("release-group identifier agrees"));
    } else if release_group_conflicts {
        score -= 500;
        reasons.push(reason("release-group identifier conflicts"));
    }

    let album_artist_ids_agree = common_ids(&inspection.tracks, |track| &track.album_artist_ids)
        .is_some_and(|ids| credit_contains_ids(&candidate.album_artist, &ids));
    if album_artist_ids_agree {
        score += 180;
        reasons.push(reason("album-artist identifiers agree"));
    }
    if let Some(year) = common_value(&inspection.tracks, |track| track.original_year)
        && Some(year) == candidate.original_year
    {
        score += 10;
        reasons.push(reason("original year agrees"));
    }
    if inspection.kind == crate::domain::SourceKind::LooseFile
        && inspection
            .tracks
            .first()
            .and_then(|track| track.title.as_ref())
            .is_some_and(|title| normalize(title) == normalize(&candidate.title))
    {
        score += 60;
        reasons.push(reason("single title agrees with the selected track"));
    }

    let count_is_compatible = !inspection.kind.requires_complete_release()
        || inspection.tracks.len() == candidate.tracks.len();
    let complete = count_is_compatible && mappings.len() == inspection.tracks.len();
    if complete {
        reasons.push(reason(format!(
            "all {} source {} {} uniquely using identifiers, positions, or title-and-duration evidence",
            inspection.tracks.len(),
            if inspection.tracks.len() == 1 {
                "track"
            } else {
                "tracks"
            },
            if inspection.tracks.len() == 1 { "maps" } else { "map" }
        )));
    }

    let meaningful_track_evidence = mappings.iter().any(|mapping| {
        matches!(
            mapping.basis,
            TrackMatchBasis::RecordingId | TrackMatchBasis::TitleAndDuration
        )
    });
    let identifier_conflict = release_group_conflicts
        || mappings.iter().any(|mapping| {
            let source = &inspection.tracks[mapping.source_index];
            let target = &candidate.tracks[mapping.candidate_index];
            source.recording_id.is_some()
                && target.recording_id.is_some()
                && source.recording_id != target.recording_id
        });
    let album_text_identity = common_string(&inspection.tracks, |track| &track.album)
        .is_some_and(|album| normalize(&album) == normalize(&candidate.title))
        && common_string(&inspection.tracks, |track| &track.album_artist)
            .is_some_and(|artist| normalize(&artist) == normalize(&candidate.album_artist.display));
    let album_title_and_track_artist_identity =
        common_string(&inspection.tracks, |track| &track.album)
            .is_some_and(|album| normalize(&album) == normalize(&candidate.title))
            && common_string(&inspection.tracks, |track| &track.artist).is_some_and(|artist| {
                normalize(&artist) == normalize(&candidate.album_artist.display)
            });
    let loose_text_identity = inspection.kind == crate::domain::SourceKind::LooseFile
        && mappings.first().is_some_and(|mapping| {
            let source = &inspection.tracks[mapping.source_index];
            let target = &candidate.tracks[mapping.candidate_index];
            source
                .title
                .as_ref()
                .is_some_and(|title| normalize(title) == normalize(&target.title))
                && source.artist.as_ref().is_some_and(|artist| {
                    normalize(artist) == normalize(&target.artist_credit.display)
                })
        });
    let all_recording_ids_agree = !mappings.is_empty()
        && mappings
            .iter()
            .all(|mapping| mapping.basis == TrackMatchBasis::RecordingId);
    let credible_identity = release_group_agrees
        || album_artist_ids_agree
        || all_recording_ids_agree
        || album_text_identity
        || album_title_and_track_artist_identity
        || loose_text_identity;

    RankedCandidate {
        candidate,
        mappings,
        reasons,
        score,
        complete,
        credible_identity,
        meaningful_track_evidence,
        identifier_conflict,
    }
}

fn best_track<'a>(
    source: &InspectedTrack,
    available: &[(usize, &'a ReleaseTrack)],
) -> Option<(usize, &'a ReleaseTrack, TrackMatchBasis)> {
    let exact_id: Vec<_> = source
        .recording_id
        .as_ref()
        .map_or_else(Vec::new, |source_id| {
            available
                .iter()
                .filter(|(_, track)| track.recording_id.as_ref() == Some(source_id))
                .copied()
                .collect()
        });
    if exact_id.len() == 1 {
        return exact_id
            .into_iter()
            .next()
            .map(|(index, track)| (index, track, TrackMatchBasis::RecordingId));
    }

    let text_and_duration: Vec<_> = source.title.as_ref().map_or_else(Vec::new, |title| {
        available
            .iter()
            .filter(|(_, track)| {
                normalize(title) == normalize(&track.title)
                    && duration_compatible(source.duration_ms, track.duration_ms)
            })
            .copied()
            .collect()
    });
    if text_and_duration.len() == 1 {
        return text_and_duration
            .into_iter()
            .next()
            .map(|(index, track)| (index, track, TrackMatchBasis::TitleAndDuration));
    }

    let exact_position: Vec<_> = source.position.map_or_else(Vec::new, |position| {
        available
            .iter()
            .filter(|(_, track)| track.position == position)
            .copied()
            .collect()
    });
    if exact_position.len() == 1 {
        exact_position
            .into_iter()
            .next()
            .map(|(index, track)| (index, track, TrackMatchBasis::PositionFallback))
    } else {
        None
    }
}

fn duration_compatible(source_ms: u64, candidate_ms: u64) -> bool {
    candidate_ms == 0 || source_ms == 0 || source_ms.abs_diff(candidate_ms) <= 7_000
}

struct TrackEvidence {
    score: i32,
    reasons: Vec<MatchReason>,
}

fn track_evidence(source: &InspectedTrack, target: &ReleaseTrack) -> TrackEvidence {
    let mut score = 0;
    let mut reasons = Vec::new();

    if source.recording_id.is_some() && source.recording_id == target.recording_id {
        score += 400;
        reasons.push(reason(format!(
            "{} has the same recording identifier",
            source.source_name
        )));
    }
    if source.position == Some(target.position) {
        score += 40;
    }
    if source
        .title
        .as_ref()
        .is_some_and(|title| normalize(title) == normalize(&target.title))
    {
        score += 30;
    }
    if source
        .artist
        .as_ref()
        .is_some_and(|artist| normalize(artist) == normalize(&target.artist_credit.display))
    {
        score += 20;
    }
    if !source.artist_ids.is_empty()
        && credit_contains_ids(&target.artist_credit, &source.artist_ids)
    {
        score += 150;
        reasons.push(reason(format!(
            "{} has matching track-artist identifiers",
            source.source_name
        )));
    }

    if target.duration_ms > 0 && source.duration_ms > 0 {
        let duration_difference = source.duration_ms.abs_diff(target.duration_ms);
        if duration_difference <= 2_000 {
            score += 30;
        } else if duration_difference <= 5_000 {
            score += 15;
        } else {
            score -= 20;
        }
    }

    TrackEvidence { score, reasons }
}

fn common_ids<F>(tracks: &[InspectedTrack], value: F) -> Option<Vec<String>>
where
    F: Fn(&InspectedTrack) -> &[String],
{
    let first = value(tracks.first()?);
    (!first.is_empty() && tracks.iter().all(|track| value(track) == first)).then(|| first.to_vec())
}

fn credit_contains_ids(credit: &crate::domain::ArtistCredit, ids: &[String]) -> bool {
    let candidate = credit
        .artists
        .iter()
        .filter_map(|artist| artist.musicbrainz_id.as_ref())
        .collect::<BTreeSet<_>>();
    !ids.is_empty() && ids.iter().all(|id| candidate.contains(id))
}

fn common_value<T, F>(tracks: &[InspectedTrack], value: F) -> Option<T>
where
    T: Copy + PartialEq,
    F: Fn(&InspectedTrack) -> Option<T>,
{
    let first = tracks.first().and_then(&value)?;
    tracks
        .iter()
        .all(|track| value(track) == Some(first))
        .then_some(first)
}

fn common_string<F>(tracks: &[InspectedTrack], value: F) -> Option<String>
where
    F: Fn(&InspectedTrack) -> &Option<String>,
{
    let first = value(tracks.first()?).as_ref()?;
    tracks
        .iter()
        .all(|track| {
            value(track)
                .as_ref()
                .is_some_and(|candidate| normalize(candidate) == normalize(first))
        })
        .then(|| first.clone())
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn reason(summary: impl Into<String>) -> MatchReason {
    MatchReason {
        summary: summary.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtistCredit, Position, ReleaseKind, SourceKind};

    fn inspected_album() -> Inspection {
        Inspection {
            source_label: "incoming/album".into(),
            kind: SourceKind::AlbumDirectory,
            tracks: vec![
                inspected_track("01 old.flac", "Opening", 1, 180_000),
                inspected_track("02 old.flac", "Closing", 2, 240_000),
            ],
        }
    }

    fn inspected_track(name: &str, title: &str, track: u16, duration_ms: u64) -> InspectedTrack {
        InspectedTrack {
            source_name: name.into(),
            title: Some(title.into()),
            artist: Some("The Group".into()),
            album: Some("The Album".into()),
            album_artist: Some("The Group".into()),
            artist_ids: Vec::new(),
            album_artist_ids: Vec::new(),
            compilation: Some(false),
            original_year: Some(1971),
            position: Some(Position::new(1, track)),
            duration_ms,
            recording_id: None,
            release_group_id: None,
        }
    }

    fn candidate(key: &str, title: &str, durations: [u64; 2]) -> CandidateRelease {
        CandidateRelease {
            provider_key: key.into(),
            title: title.into(),
            album_artist: ArtistCredit::single("The Group"),
            original_year: Some(1971),
            kind: ReleaseKind::Album,
            tracks: vec![
                ReleaseTrack {
                    title: "Opening".into(),
                    artist_credit: ArtistCredit::single("The Group"),
                    position: Position::new(1, 1),
                    duration_ms: durations[0],
                    recording_id: None,
                },
                ReleaseTrack {
                    title: "Closing".into(),
                    artist_credit: ArtistCredit::single("The Group"),
                    position: Position::new(1, 2),
                    duration_ms: durations[1],
                    recording_id: None,
                },
            ],
            release_group_id: None,
            exact_release_id: None,
        }
    }

    #[test]
    fn auto_selects_clear_structural_text_and_duration_match() {
        let decision = MatchPolicy::default().decide(
            &inspected_album(),
            vec![
                candidate("best", "The Album", [180_500, 239_500]),
                candidate("other", "Other Edition", [191_000, 251_000]),
            ],
        );

        let MatchDecision::Selected { selected, .. } = decision else {
            panic!("expected automatic selection");
        };
        assert_eq!(selected.candidate.provider_key, "best");
        assert!(selected.is_complete());
    }

    #[test]
    fn asks_when_two_candidates_have_equivalent_evidence() {
        let decision = MatchPolicy::default().decide(
            &inspected_album(),
            vec![
                candidate("one", "The Album", [180_000, 240_000]),
                candidate("two", "The Album", [180_000, 240_000]),
            ],
        );

        let MatchDecision::NeedsChoice(candidates) = decision else {
            panic!("expected a choice");
        };
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn rejects_album_candidate_with_incomplete_structure() {
        let mut incomplete = candidate("short", "The Album", [180_000, 240_000]);
        incomplete.tracks.pop();

        let decision = MatchPolicy::default().decide(&inspected_album(), vec![incomplete]);

        assert!(matches!(decision, MatchDecision::NoUsableMatch(_)));
    }

    #[test]
    fn ignores_incomplete_candidate_when_selecting_the_best_usable_match() {
        let mut incomplete = candidate("incomplete", "The Album", [180_000, 240_000]);
        incomplete.tracks.pop();
        incomplete.tracks[0].recording_id = Some("recording-1".into());

        let mut inspection = inspected_album();
        inspection.tracks[0].recording_id = Some("recording-1".into());
        let usable = candidate("usable", "The Album", [180_500, 239_500]);

        let decision = MatchPolicy::default().decide(&inspection, vec![incomplete, usable]);

        let MatchDecision::Selected { selected, .. } = decision else {
            panic!("expected the complete candidate to be selected");
        };
        assert_eq!(selected.candidate.provider_key, "usable");
    }

    #[test]
    fn weak_position_only_evidence_is_not_automatically_accepted() {
        let mut weak = candidate("weak", "Different Album", [300_000, 300_000]);
        for (index, track) in weak.tracks.iter_mut().enumerate() {
            track.title = format!("Unrelated {index}");
            track.artist_credit = ArtistCredit::single("Someone Else");
        }

        let decision = MatchPolicy::default().decide(&inspected_album(), vec![weak]);

        assert!(matches!(decision, MatchDecision::NeedsChoice(_)));
    }

    #[test]
    fn standalone_track_can_map_into_a_single_release() {
        let inspection = Inspection {
            source_label: "song.flac".into(),
            kind: SourceKind::LooseFile,
            tracks: vec![InspectedTrack {
                source_name: "song.flac".into(),
                title: Some("The Song".into()),
                artist: Some("The Artist".into()),
                album: None,
                album_artist: None,
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: None,
                original_year: None,
                position: None,
                duration_ms: 200_000,
                recording_id: Some("recording-1".into()),
                release_group_id: None,
            }],
        };
        let candidate = CandidateRelease {
            provider_key: "single".into(),
            title: "The Song".into(),
            album_artist: ArtistCredit::single("The Artist"),
            original_year: Some(1982),
            kind: ReleaseKind::Single,
            tracks: vec![ReleaseTrack {
                title: "The Song".into(),
                artist_credit: ArtistCredit::single("The Artist"),
                position: Position::new(1, 1),
                duration_ms: 200_500,
                recording_id: Some("recording-1".into()),
            }],
            release_group_id: Some("group-1".into()),
            exact_release_id: None,
        };

        let decision = MatchPolicy::default().decide(&inspection, vec![candidate]);

        assert!(matches!(decision, MatchDecision::Selected { .. }));
    }

    #[test]
    fn loose_track_asks_when_a_single_and_album_are_both_credible() {
        let inspection = Inspection {
            source_label: "song.flac".into(),
            kind: SourceKind::LooseFile,
            tracks: vec![InspectedTrack {
                source_name: "song.flac".into(),
                title: Some("The Song".into()),
                artist: Some("The Artist".into()),
                album: None,
                album_artist: None,
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: None,
                original_year: None,
                position: None,
                duration_ms: 200_000,
                recording_id: None,
                release_group_id: None,
            }],
        };
        let release = |key: &str, kind: ReleaseKind| CandidateRelease {
            provider_key: key.into(),
            title: "The Song".into(),
            album_artist: ArtistCredit::single("The Artist"),
            original_year: Some(1982),
            kind,
            tracks: vec![ReleaseTrack {
                title: "The Song".into(),
                artist_credit: ArtistCredit::single("The Artist"),
                position: Position::new(1, 1),
                duration_ms: 200_500,
                recording_id: None,
            }],
            release_group_id: Some(format!("group-{key}")),
            exact_release_id: None,
        };

        let decision = MatchPolicy::default().decide(
            &inspection,
            vec![
                release("album", ReleaseKind::Album),
                release("single", ReleaseKind::Single),
            ],
        );

        let MatchDecision::NeedsChoice(candidates) = decision else {
            panic!("expected a choice");
        };
        assert_eq!(candidates[0].candidate.provider_key, "single");
    }

    #[test]
    fn unrelated_single_does_not_displace_a_credible_album() {
        let mut inspection = inspected_album();
        inspection.kind = SourceKind::LooseFile;
        inspection.tracks.truncate(1);
        let album = candidate("album", "The Album", [180_000, 240_000]);
        let mut single = candidate("single", "Unrelated", [300_000, 240_000]);
        single.kind = ReleaseKind::Single;
        single.tracks.truncate(1);
        single.tracks[0].title = "Not Opening".into();

        let decision = MatchPolicy::default().decide(&inspection, vec![single, album]);

        let MatchDecision::Selected { selected, .. } = decision else {
            panic!("expected the credible album");
        };
        assert_eq!(selected.candidate.provider_key, "album");
    }

    #[test]
    fn album_length_cannot_turn_position_only_evidence_into_auto_acceptance() {
        let mut inspection = inspected_album();
        let mut weak = candidate("weak", "Different", [300_000, 300_000]);
        for index in 2..20 {
            inspection.tracks.push(inspected_track(
                &format!("{index:02}.flac"),
                &format!("Source {index}"),
                index as u16 + 1,
                180_000,
            ));
            weak.tracks.push(ReleaseTrack {
                title: format!("Candidate {index}"),
                artist_credit: ArtistCredit::single("Elsewhere"),
                position: Position::new(1, index as u16 + 1),
                duration_ms: 300_000,
                recording_id: None,
            });
        }

        let decision = MatchPolicy::default().decide(&inspection, vec![weak]);

        assert!(matches!(decision, MatchDecision::NeedsChoice(_)));
    }

    #[test]
    fn unique_title_and_duration_override_incorrect_positions() {
        let mut swapped = candidate("swapped", "The Album", [180_000, 240_000]);
        swapped.tracks[0].position = Position::new(1, 2);
        swapped.tracks[1].position = Position::new(1, 1);

        let decision = MatchPolicy::default().decide(&inspected_album(), vec![swapped]);

        let MatchDecision::Selected { selected, .. } = decision else {
            panic!("expected a strong textual mapping");
        };
        assert!(
            selected
                .mappings
                .iter()
                .all(|mapping| mapping.basis == TrackMatchBasis::TitleAndDuration)
        );
    }

    #[test]
    fn conflicting_recording_identifier_prevents_automatic_acceptance() {
        let mut inspection = inspected_album();
        inspection.tracks[0].recording_id = Some("source-recording".into());
        let mut release = candidate("conflict", "The Album", [180_000, 240_000]);
        release.tracks[0].recording_id = Some("different-recording".into());

        let decision = MatchPolicy::default().decide(&inspection, vec![release]);

        assert!(matches!(decision, MatchDecision::NeedsChoice(_)));
    }

    #[test]
    fn missing_provider_duration_is_not_treated_as_a_mismatch() {
        let mut release = candidate("no-duration", "The Album", [0, 0]);
        for track in &mut release.tracks {
            track.duration_ms = 0;
        }

        let decision = MatchPolicy::default().decide(&inspected_album(), vec![release]);

        assert!(matches!(decision, MatchDecision::Selected { .. }));
    }

    #[test]
    fn coherent_album_artist_identifiers_can_confirm_a_collaboration_credit() {
        let mut inspection = inspected_album();
        for track in &mut inspection.tracks {
            track.album_artist = Some("Abbreviated Duo".into());
            track.album_artist_ids = vec!["artist-a".into(), "artist-b".into()];
        }
        let mut release = candidate("duo", "The Album", [180_000, 240_000]);
        release.album_artist = crate::domain::ArtistCredit::credited(
            "Alice with Bob",
            vec![
                crate::domain::Artist {
                    name: "Alice".into(),
                    musicbrainz_id: Some("artist-a".into()),
                },
                crate::domain::Artist {
                    name: "Bob".into(),
                    musicbrainz_id: Some("artist-b".into()),
                },
            ],
        );

        let decision = MatchPolicy::default().decide(&inspection, vec![release]);

        assert!(matches!(decision, MatchDecision::Selected { .. }));
    }

    #[test]
    fn coherent_track_artist_can_confirm_album_identity_when_album_artist_is_missing() {
        let mut inspection = inspected_album();
        for track in &mut inspection.tracks {
            track.album_artist = None;
        }

        let decision = MatchPolicy::default().decide(
            &inspection,
            vec![candidate("album", "The Album", [180_000, 240_000])],
        );

        assert!(matches!(decision, MatchDecision::Selected { .. }));
    }

    #[test]
    fn normalization_handles_case_punctuation_and_spacing() {
        assert_eq!(normalize("  AC/DC—Live  "), "ac dc live");
    }
}
