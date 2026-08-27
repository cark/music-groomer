use std::collections::BTreeSet;
use std::io;

use crate::domain::{Inspection, SourceKind};
use crate::matching::{MatchDecision, RankedCandidate};
use crate::terminal::{Interaction, TextStyle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataSelection {
    Provider(Box<RankedCandidate>),
    ExistingTags,
    Cancelled,
}

pub fn choose(
    interaction: &mut impl Interaction,
    inspection: &Inspection,
    decision: MatchDecision,
) -> io::Result<MetadataSelection> {
    match decision {
        MatchDecision::Selected { selected, .. } => {
            interaction.show(&format!(
                "\n{} {}",
                interaction.styled(TextStyle::Success, "Clear metadata match:"),
                interaction.styled(TextStyle::Value, &selected.candidate.human_label())
            ))?;
            Ok(MetadataSelection::Provider(selected))
        }
        MatchDecision::NeedsChoice(candidates) => {
            choose_candidate(interaction, inspection, candidates)
        }
        MatchDecision::NoUsableMatch(_) => choose_fallback(interaction, inspection),
    }
}

fn choose_candidate(
    interaction: &mut impl Interaction,
    inspection: &Inspection,
    candidates: Vec<RankedCandidate>,
) -> io::Result<MetadataSelection> {
    interaction.show(&format!(
        "\n{}",
        interaction.styled(TextStyle::Warning, "Metadata needs your choice")
    ))?;
    if candidates.len() == 1 {
        return confirm_only_candidate(interaction, inspection, &candidates[0]);
    }
    let mut shown = candidates.len().min(3);
    loop {
        show_candidates(interaction, &candidates, shown)?;
        let mut actions = vec!["a number".to_owned()];
        if shown < candidates.len() {
            actions.push("[m] Show more".into());
        }
        actions.push("[t] Track-list details".into());
        if coherent_existing_metadata(inspection).is_ok() {
            actions.push("[e] Use existing tags (unverified)".into());
        }
        actions.push("[c] Cancel".into());
        let answer = interaction.ask(&format!("Choose {}: ", actions.join("  ")))?;
        if let Ok(index) = answer.parse::<usize>()
            && (1..=shown).contains(&index)
        {
            return Ok(MetadataSelection::Provider(Box::new(
                candidates[index - 1].clone(),
            )));
        }
        match answer.to_ascii_lowercase().as_str() {
            "m" | "more" if shown < candidates.len() => shown = candidates.len(),
            "t" | "tracks" | "details" => show_track_lists(interaction, &candidates[..shown])?,
            "e" | "existing" if coherent_existing_metadata(inspection).is_ok() => {
                return Ok(MetadataSelection::ExistingTags);
            }
            "c" | "cancel" | "q" | "quit" => return Ok(MetadataSelection::Cancelled),
            _ => interaction.show("Please choose one of the displayed actions.")?,
        }
    }
}

fn confirm_only_candidate(
    interaction: &mut impl Interaction,
    inspection: &Inspection,
    candidate: &RankedCandidate,
) -> io::Result<MetadataSelection> {
    show_candidates(interaction, std::slice::from_ref(candidate), 1)?;
    loop {
        let existing = if coherent_existing_metadata(inspection).is_ok() {
            "  [e] Existing tags"
        } else {
            ""
        };
        let answer = interaction.ask(&format!(
            "Use this uncertain match? [y/N]  [t] Track-list details{existing}: "
        ))?;
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => {
                return Ok(MetadataSelection::Provider(Box::new(candidate.clone())));
            }
            "t" | "tracks" | "details" => {
                show_track_lists(interaction, std::slice::from_ref(candidate))?;
            }
            "e" | "existing" if coherent_existing_metadata(inspection).is_ok() => {
                return Ok(MetadataSelection::ExistingTags);
            }
            "" | "n" | "no" | "c" | "cancel" | "q" | "quit" => {
                return Ok(MetadataSelection::Cancelled);
            }
            _ => interaction.show("Please answer Yes, No, Details, or Existing tags.")?,
        }
    }
}

pub fn revise(
    interaction: &mut impl Interaction,
    inspection: &Inspection,
    candidates: &[RankedCandidate],
    current: &MetadataSelection,
) -> io::Result<MetadataSelection> {
    interaction.show("\nMetadata selection:")?;
    match current {
        MetadataSelection::Provider(selected) => {
            interaction.show(&format!("  Current: {}", selected.candidate.human_label()))?;
            interaction.show("  Evidence:")?;
            for reason in &selected.reasons {
                interaction.show(&format!("    - {}", reason.summary))?;
            }
            interaction.show(&format!(
                "  MusicBrainz release group: {}",
                selected
                    .candidate
                    .release_group_id
                    .as_deref()
                    .unwrap_or("unknown")
            ))?;
        }
        MetadataSelection::ExistingTags => {
            interaction.show("  Current: existing source tags (unverified)")?
        }
        MetadataSelection::Cancelled => return Ok(current.clone()),
    }
    if candidates.is_empty() {
        interaction.show("  No usable provider alternatives are available.")?;
    } else {
        show_candidates(interaction, candidates, candidates.len())?;
    }
    loop {
        let existing = if coherent_existing_metadata(inspection).is_ok() {
            "  [e] Existing tags"
        } else {
            ""
        };
        let answer = interaction.ask(&format!(
            "Choose a number to change, [t] Track-list details{existing}  [b] Back: "
        ))?;
        if let Ok(index) = answer.parse::<usize>()
            && (1..=candidates.len()).contains(&index)
        {
            return Ok(MetadataSelection::Provider(Box::new(
                candidates[index - 1].clone(),
            )));
        }
        match answer.to_ascii_lowercase().as_str() {
            "t" | "tracks" | "details" => show_track_lists(interaction, candidates)?,
            "e" | "existing" if coherent_existing_metadata(inspection).is_ok() => {
                return Ok(MetadataSelection::ExistingTags);
            }
            "" | "b" | "back" => return Ok(current.clone()),
            _ => interaction.show("Please choose a displayed metadata action.")?,
        }
    }
}

fn show_candidates(
    interaction: &mut impl Interaction,
    candidates: &[RankedCandidate],
    shown: usize,
) -> io::Result<()> {
    for (index, candidate) in candidates.iter().take(shown).enumerate() {
        interaction.show(&format!(
            "  {}. {}",
            index + 1,
            candidate.candidate.human_label()
        ))?;
        interaction.show(&format!("     {}", track_list_summary(candidate)))?;
    }
    Ok(())
}

fn track_list_summary(candidate: &RankedCandidate) -> String {
    let titles = candidate
        .candidate
        .tracks
        .iter()
        .take(3)
        .map(|track| track.title.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    if candidate.candidate.tracks.len() > 3 {
        format!("Tracks: {titles}; …")
    } else {
        format!("Tracks: {titles}")
    }
}

fn show_track_lists(
    interaction: &mut impl Interaction,
    candidates: &[RankedCandidate],
) -> io::Result<()> {
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        interaction.show(&format!(
            "\n  {}. {}",
            candidate_index + 1,
            candidate.candidate.human_label()
        ))?;
        for track in &candidate.candidate.tracks {
            interaction.show(&format!(
                "     {}-{:02} {} — {}",
                track.position.disc, track.position.track, track.title, track.artist_credit.display
            ))?;
        }
    }
    Ok(())
}

fn choose_fallback(
    interaction: &mut impl Interaction,
    inspection: &Inspection,
) -> io::Result<MetadataSelection> {
    match coherent_existing_metadata(inspection) {
        Ok(()) => {
            interaction.show(&format!(
                "\n{}",
                interaction.styled(
                    TextStyle::Warning,
                    "No defensible MusicBrainz match was found."
                )
            ))?;
            loop {
                let answer = interaction.ask(
                    "Use the internally coherent existing tags as unverified metadata? [Y/n]: ",
                )?;
                match answer.to_ascii_lowercase().as_str() {
                    "" | "y" | "yes" => return Ok(MetadataSelection::ExistingTags),
                    "n" | "no" | "c" | "cancel" => return Ok(MetadataSelection::Cancelled),
                    _ => interaction.show("Please answer Yes or No.")?,
                }
            }
        }
        Err(problems) => {
            interaction.show(&format!(
                "\n{} No provider match is available and existing metadata is incomplete:",
                interaction.styled(TextStyle::Error, "Cannot build reliable metadata.")
            ))?;
            for problem in problems {
                interaction.show(&format!("  - {problem}"))?;
            }
            Ok(MetadataSelection::Cancelled)
        }
    }
}

pub fn coherent_existing_metadata(inspection: &Inspection) -> Result<(), Vec<String>> {
    let mut problems = BTreeSet::new();
    if inspection.tracks.is_empty() {
        problems.insert("no audio tracks".to_owned());
    }
    for track in &inspection.tracks {
        if blank(&track.title) {
            problems.insert("a track title is missing".to_owned());
        }
        if blank(&track.artist) {
            problems.insert("a track artist is missing".to_owned());
        }
        if inspection.kind == SourceKind::AlbumDirectory {
            if blank(&track.album) {
                problems.insert("an album title is missing".to_owned());
            }
            if blank(&track.album_artist) {
                problems.insert("an album artist is missing".to_owned());
            }
            if track.position.is_none() {
                problems.insert("a disc or track number is missing".to_owned());
            }
        }
    }
    if inspection.kind == SourceKind::AlbumDirectory {
        common_or_problem(
            inspection,
            |track| track.album.as_deref(),
            "album titles disagree",
            &mut problems,
        );
        common_or_problem(
            inspection,
            |track| track.album_artist.as_deref(),
            "album artists disagree",
            &mut problems,
        );
        let positions = inspection
            .tracks
            .iter()
            .filter_map(|track| track.position)
            .collect::<BTreeSet<_>>();
        if positions.len() != inspection.tracks.len() {
            problems.insert("disc-track positions are duplicated or missing".to_owned());
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.into_iter().collect())
    }
}

fn common_or_problem(
    inspection: &Inspection,
    value: impl Fn(&crate::domain::InspectedTrack) -> Option<&str>,
    problem: &str,
    problems: &mut BTreeSet<String>,
) {
    let values = inspection
        .tracks
        .iter()
        .filter_map(value)
        .collect::<Vec<_>>();
    if let Some(first) = values.first()
        && !values
            .iter()
            .all(|value| normalized_field(value) == normalized_field(first))
    {
        problems.insert(problem.to_owned());
    }
}

fn normalized_field(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|value| value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::domain::{
        ArtistCredit, CandidateRelease, InspectedTrack, Position, ReleaseKind, ReleaseTrack,
    };
    use crate::matching::MatchPolicy;
    use crate::terminal::Interaction;

    struct Scripted {
        answers: VecDeque<String>,
        transcript: String,
    }

    impl Interaction for Scripted {
        fn show(&mut self, text: &str) -> io::Result<()> {
            self.transcript.push_str(text);
            self.transcript.push('\n');
            Ok(())
        }

        fn ask(&mut self, prompt: &str) -> io::Result<String> {
            self.transcript.push_str(prompt);
            Ok(self.answers.pop_front().unwrap_or_default())
        }
    }

    #[test]
    fn incomplete_local_metadata_explains_why_it_cannot_be_fallback() {
        let inspection = Inspection {
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
                duration_ms: 1000,
                recording_id: None,
                release_group_id: None,
            }],
        };
        let mut interaction = Scripted {
            answers: VecDeque::new(),
            transcript: String::new(),
        };

        let selection = choose(
            &mut interaction,
            &inspection,
            MatchDecision::NoUsableMatch(Vec::new()),
        )
        .unwrap();

        assert_eq!(selection, MetadataSelection::Cancelled);
        assert!(interaction.transcript.contains("track artist is missing"));
        assert!(interaction.transcript.contains("track title is missing"));
    }

    #[test]
    fn coherent_standalone_can_use_existing_tags() {
        let inspection = Inspection {
            source_label: "track.flac".into(),
            kind: SourceKind::LooseFile,
            tracks: vec![InspectedTrack {
                source_name: "track.flac".into(),
                title: Some("Title".into()),
                artist: Some("Artist".into()),
                album: None,
                album_artist: None,
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: None,
                original_year: None,
                position: Some(Position::new(1, 1)),
                duration_ms: 1000,
                recording_id: None,
                release_group_id: None,
            }],
        };

        assert!(coherent_existing_metadata(&inspection).is_ok());
    }

    #[test]
    fn sole_uncertain_candidate_uses_confirmation_instead_of_numbered_choice() {
        let inspection = Inspection {
            source_label: "album".into(),
            kind: SourceKind::AlbumDirectory,
            tracks: vec![InspectedTrack {
                source_name: "01.flac".into(),
                title: Some("Track".into()),
                artist: Some("Artist".into()),
                album: Some("Release".into()),
                album_artist: Some("Artist".into()),
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: Some(false),
                original_year: Some(2000),
                position: Some(Position::new(1, 1)),
                duration_ms: 120_000,
                recording_id: None,
                release_group_id: None,
            }],
        };
        let candidate = CandidateRelease {
            provider_key: "candidate".into(),
            title: "Release".into(),
            album_artist: ArtistCredit::single("Artist"),
            original_year: Some(2000),
            kind: ReleaseKind::Other("Broadcast".into()),
            tracks: vec![ReleaseTrack {
                title: "Track".into(),
                artist_credit: ArtistCredit::single("Artist"),
                position: Position::new(1, 1),
                duration_ms: 120_000,
                recording_id: None,
            }],
            release_group_id: Some("group".into()),
            exact_release_id: None,
        };
        let mut interaction = Scripted {
            answers: VecDeque::from(["y".into()]),
            transcript: String::new(),
        };

        let selection = choose(
            &mut interaction,
            &inspection,
            MatchPolicy::default().decide(&inspection, vec![candidate]),
        )
        .unwrap();

        assert!(matches!(selection, MetadataSelection::Provider(_)));
        assert!(interaction.transcript.contains("Use this uncertain match?"));
        assert!(!interaction.transcript.contains("Choose a number"));
    }
}
