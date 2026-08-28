use std::collections::BTreeSet;

use crate::guided_matching::GuidedMatchResult;
use crate::plan::{GroomingPlan, PlanWarning};
use crate::source::{InspectionNotice, NoticeKind, SourceInspection};

pub(super) fn for_plan(
    source: &SourceInspection,
    matched: &GuidedMatchResult,
    plan: &GroomingPlan,
) -> Vec<PlanWarning> {
    let resolved = source
        .notices
        .iter()
        .filter(|notice| resolved_by_plan(notice, plan))
        .map(InspectionNotice::summary)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut warnings = matched
        .warnings
        .iter()
        .filter(|warning| !resolved.contains(*warning) && seen.insert((*warning).clone()))
        .map(|warning| PlanWarning {
            summary: warning.clone(),
            detail: warning.clone(),
        })
        .collect::<Vec<_>>();
    add_stale_references(source, plan, &mut warnings);
    warnings
}

fn resolved_by_plan(notice: &InspectionNotice, plan: &GroomingPlan) -> bool {
    notice.kind == NoticeKind::MissingMetadata
        && plan.tracks.iter().all(|track| track.planned_tags.is_some())
}

fn add_stale_references(
    source: &SourceInspection,
    plan: &GroomingPlan,
    warnings: &mut Vec<PlanWarning>,
) {
    let audio_paths_change = plan.tracks.iter().any(|track| {
        track.destination.strip_prefix(&plan.destination).ok()
            != Some(track.source_relative.as_path())
    });
    if !audio_paths_change {
        return;
    }
    for file in plan.ancillary.iter().filter(|file| {
        ["cue", "m3u", "m3u8"].iter().any(|extension| {
            file.source_relative
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
    }) {
        let already_warned = source.notices.iter().any(|notice| {
            notice.kind == NoticeKind::StaleReference
                && notice.path.as_ref() == Some(&file.source_relative)
        });
        if !already_warned {
            let path = file.source_relative.display();
            let summary =
                format!("{path}: planned audio renames may leave preserved references stale");
            warnings.push(PlanWarning {
                summary: summary.clone(),
                detail: summary,
            });
        }
    }
}
