use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::domain::{
    Artist, ArtistCredit, CandidateRelease, InspectedTrack, Inspection, Position, ReleaseKind,
    ReleaseTrack, SourceKind,
};
use crate::layout::{LayoutPolicy, LayoutTrack, ReleaseLayout, StandaloneLayout};
use crate::matching::{MatchDecision, MatchPolicy, RankedCandidate};
use crate::plan::{
    ApplyReport, ArtworkChoice, ArtworkOrigin, GroomingPlan, MatchSelection, MetadataBasis,
    PlanWarning, TagChange, TagField, TrackPlan,
};

const DEFAULT_DEMO_OUTPUT: &str = "/tmp/music-groomer-demo-output";

pub trait Interaction {
    fn show(&mut self, text: &str) -> io::Result<()>;
    fn ask(&mut self, prompt: &str) -> io::Result<String>;
}

pub struct StdioInteraction<R, W> {
    input: R,
    output: W,
}

impl<R, W> StdioInteraction<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self { input, output }
    }
}

impl<R: BufRead, W: Write> Interaction for StdioInteraction<R, W> {
    fn show(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.output, "{text}")
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        write!(self.output, "{prompt}")?;
        self.output.flush()?;
        let mut answer = String::new();
        self.input.read_line(&mut answer)?;
        Ok(answer.trim().to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoScenario {
    ConfidentAlbum,
    AmbiguousCollaboration,
    MatchedSingle,
    StandaloneTrack,
}

impl DemoScenario {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "confident" => Some(Self::ConfidentAlbum),
            "ambiguous" => Some(Self::AmbiguousCollaboration),
            "matched-single" => Some(Self::MatchedSingle),
            "standalone" => Some(Self::StandaloneTrack),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemoOutcome {
    Cancelled,
    Applied(ApplyReport),
}

#[derive(Debug)]
pub enum DemoError {
    Io(io::Error),
    InvalidDemoData(String),
}

impl fmt::Display for DemoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "terminal interaction failed: {error}"),
            Self::InvalidDemoData(message) => write!(formatter, "invalid demo data: {message}"),
        }
    }
}

impl std::error::Error for DemoError {}

impl From<io::Error> for DemoError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn run(
    interaction: &mut impl Interaction,
    scenario: Option<DemoScenario>,
    output_root: Option<&Path>,
) -> Result<DemoOutcome, DemoError> {
    interaction.show("music-groomer — guided preview demo")?;
    interaction
        .show("Simulation only: no files are read or written, and no network requests are made.")?;
    interaction.show("")?;

    let scenario = match scenario {
        Some(scenario) => scenario,
        None => match choose_scenario(interaction)? {
            Some(scenario) => scenario,
            None => {
                interaction.show("Cancelled. No files were written.")?;
                return Ok(DemoOutcome::Cancelled);
            }
        },
    };
    let data = demo_data(scenario);

    show_inspection(interaction, &data)?;
    let (selected, match_selection) = match select_metadata(interaction, &data)? {
        MetadataSelection::Matched {
            candidate,
            automatic,
        } => (
            Some(candidate),
            if automatic {
                MatchSelection::Automatic
            } else {
                MatchSelection::UserChosen
            },
        ),
        MetadataSelection::ExistingTags => (None, MatchSelection::ExistingTags),
        MetadataSelection::Cancelled => {
            interaction.show("Cancelled. No files were written.")?;
            return Ok(DemoOutcome::Cancelled);
        }
    };
    let destination_root = output_root.unwrap_or_else(|| Path::new(DEFAULT_DEMO_OUTPUT));
    let mut plan = build_plan(&data, selected, match_selection, destination_root)?;

    loop {
        show_summary(interaction, &plan)?;
        let action = interaction
            .ask("Choose: [a] Apply  [r] Review all changes  [w] Artwork  [c] Cancel: ")?
            .to_ascii_lowercase();
        match action.as_str() {
            "a" | "apply" => {
                let confirmed = interaction
                    .ask(&format!(
                        "Apply this exact plan to {}? [y/N]: ",
                        plan.destination.display()
                    ))?
                    .to_ascii_lowercase();
                if matches!(confirmed.as_str(), "y" | "yes") {
                    let report = ApplyReport {
                        destination: plan.destination.clone(),
                        tracks_validated: plan.tracks.len(),
                        artwork_validated: plan.artwork.output_name.is_some(),
                        source_unchanged: true,
                        simulated: true,
                    };
                    interaction.show("")?;
                    interaction.show("Demo apply complete. No files were written.")?;
                    interaction.show(&format!(
                        "Would validate {} track(s) at {}.",
                        report.tracks_validated,
                        report.destination.display()
                    ))?;
                    interaction.show("The source would remain untouched.")?;
                    return Ok(DemoOutcome::Applied(report));
                }
                interaction.show("Apply not confirmed; returning to the preview.")?;
            }
            "r" | "review" => show_details(interaction, &plan)?,
            "w" | "artwork" => {
                plan = choose_artwork(interaction, plan)?;
            }
            "c" | "cancel" | "q" | "quit" | "" => {
                interaction.show("Cancelled. No files were written.")?;
                return Ok(DemoOutcome::Cancelled);
            }
            _ => {
                interaction.show("Please choose Apply, Review all changes, Artwork, or Cancel.")?
            }
        }
    }
}

fn choose_scenario(interaction: &mut impl Interaction) -> Result<Option<DemoScenario>, DemoError> {
    interaction.show("Choose a pretend source to explore:")?;
    interaction.show("  1. Ordinary album with a clear match")?;
    interaction.show("  2. Collaboration album needing your choice")?;
    interaction.show("  3. Loose track matched to a single")?;
    interaction.show("  4. Loose track kept as a standalone track")?;
    loop {
        match interaction.ask("Source [1-4, or c to cancel]: ")?.as_str() {
            "1" => return Ok(Some(DemoScenario::ConfidentAlbum)),
            "2" => return Ok(Some(DemoScenario::AmbiguousCollaboration)),
            "3" => return Ok(Some(DemoScenario::MatchedSingle)),
            "4" => return Ok(Some(DemoScenario::StandaloneTrack)),
            "c" | "C" | "q" | "Q" | "" => return Ok(None),
            _ => interaction.show("Please enter 1, 2, 3, 4, or c.")?,
        }
    }
}

struct DemoData {
    inspection: Inspection,
    candidates: Vec<CandidateRelease>,
    extensions: Vec<String>,
    source_artwork: Option<ArtworkChoice>,
    provider_artwork: Option<ArtworkChoice>,
    warning: Option<PlanWarning>,
    embedded_artwork_count: usize,
}

fn show_inspection(interaction: &mut impl Interaction, data: &DemoData) -> Result<(), DemoError> {
    interaction.show("Inspection")?;
    interaction.show(&format!("  Source: {}", data.inspection.source_label))?;
    let kind = match data.inspection.kind {
        SourceKind::AlbumDirectory => "album directory",
        SourceKind::LooseFile => "one loose audio track",
    };
    interaction.show(&format!(
        "  Found: {} {} file(s); treating this as {kind}.",
        data.inspection.tracks.len(),
        data.extensions
            .first()
            .map_or("audio", |extension| extension.as_str())
            .to_ascii_uppercase()
    ))?;
    interaction.show("  Source remains read-only.")?;
    interaction.show("")?;
    Ok(())
}

fn select_metadata(
    interaction: &mut impl Interaction,
    data: &DemoData,
) -> Result<MetadataSelection, DemoError> {
    match MatchPolicy::default().decide(&data.inspection, data.candidates.clone()) {
        MatchDecision::Selected(selected) => {
            interaction.show(&format!(
                "Matched automatically: {}",
                selected.candidate.human_label()
            ))?;
            for reason in selected.reasons.iter().take(3) {
                interaction.show(&format!("  ✓ {}", reason.summary))?;
            }
            interaction.show("")?;
            Ok(MetadataSelection::Matched {
                candidate: selected,
                automatic: true,
            })
        }
        MatchDecision::NeedsChoice(candidates) => {
            interaction.show("I found more than one plausible release. Which looks right?")?;
            for (index, candidate) in candidates.iter().enumerate() {
                interaction.show(&format!(
                    "  {}. {}",
                    index + 1,
                    candidate.candidate.human_label()
                ))?;
            }
            loop {
                let answer = interaction.ask("Release number, or c to cancel: ")?;
                if matches!(answer.as_str(), "c" | "C" | "q" | "Q" | "") {
                    return Ok(MetadataSelection::Cancelled);
                }
                if let Ok(index) = answer.parse::<usize>() {
                    if let Some(candidate) = candidates.get(index.saturating_sub(1)) {
                        interaction
                            .show(&format!("Using: {}", candidate.candidate.human_label()))?;
                        interaction.show("")?;
                        return Ok(MetadataSelection::Matched {
                            candidate: Box::new(candidate.clone()),
                            automatic: false,
                        });
                    }
                }
                interaction.show("Please enter one of the displayed release numbers, or c.")?;
            }
        }
        MatchDecision::NoUsableMatch(_) => {
            if coherent_standalone(&data.inspection) {
                interaction.show("No matching single was found.")?;
                interaction.show(
                    "The existing artist and title are coherent, so this can remain a standalone track.",
                )?;
                interaction.show("")?;
                Ok(MetadataSelection::ExistingTags)
            } else {
                Err(DemoError::InvalidDemoData(
                    "no usable match or coherent standalone metadata".into(),
                ))
            }
        }
    }
}

enum MetadataSelection {
    Matched {
        candidate: Box<RankedCandidate>,
        automatic: bool,
    },
    ExistingTags,
    Cancelled,
}

fn coherent_standalone(inspection: &Inspection) -> bool {
    inspection.kind == SourceKind::LooseFile
        && inspection.tracks.len() == 1
        && inspection.tracks[0]
            .artist
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        && inspection.tracks[0]
            .title
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
}

fn build_plan(
    data: &DemoData,
    selected: Option<Box<RankedCandidate>>,
    match_selection: MatchSelection,
    output_root: &Path,
) -> Result<GroomingPlan, DemoError> {
    let (metadata, reasons, relative_layout, track_changes) = match selected {
        Some(selected) => {
            let candidate = selected.candidate.clone();
            let disc_count = candidate
                .tracks
                .iter()
                .map(|track| track.position.disc)
                .max()
                .unwrap_or(1);
            let tracks = selected
                .mappings
                .iter()
                .map(|mapping| {
                    let target = &candidate.tracks[mapping.candidate_index];
                    LayoutTrack {
                        title: target.title.clone(),
                        disc: target.position.disc,
                        track: target.position.track,
                        extension: data.extensions[mapping.source_index].clone(),
                    }
                })
                .collect();
            let layout = LayoutPolicy
                .release(&ReleaseLayout {
                    album_artist: candidate.album_artist.clone(),
                    title: candidate.title.clone(),
                    original_year: candidate.original_year,
                    disc_count,
                    tracks,
                })
                .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
            let changes = selected
                .mappings
                .iter()
                .map(|mapping| {
                    changes_for(
                        &data.inspection.tracks[mapping.source_index],
                        &candidate,
                        &candidate.tracks[mapping.candidate_index],
                    )
                })
                .collect();
            (
                MetadataBasis::MusicBrainz(candidate),
                selected
                    .reasons
                    .into_iter()
                    .map(|reason| reason.summary)
                    .collect(),
                layout,
                changes,
            )
        }
        None => {
            let source = &data.inspection.tracks[0];
            let artist = source.artist.as_ref().expect("coherence checked");
            let title = source.title.as_ref().expect("coherence checked");
            let layout = LayoutPolicy
                .standalone(&StandaloneLayout {
                    artist: ArtistCredit::single(artist),
                    title: title.clone(),
                    extension: data.extensions[0].clone(),
                })
                .map_err(|error| DemoError::InvalidDemoData(error.to_string()))?;
            (
                MetadataBasis::ExistingTags,
                vec!["existing artist and title are internally coherent".into()],
                layout,
                vec![Vec::new()],
            )
        }
    };

    let destination = output_root.join(&relative_layout.directory);
    let tracks = data
        .inspection
        .tracks
        .iter()
        .zip(relative_layout.tracks)
        .zip(track_changes)
        .map(|((source, relative_destination), tag_changes)| TrackPlan {
            source_name: source.source_name.clone(),
            destination: output_root.join(relative_destination),
            tag_changes,
        })
        .collect();
    let artwork = data
        .source_artwork
        .clone()
        .or_else(|| data.provider_artwork.clone())
        .unwrap_or_else(no_artwork);
    let artwork_alternatives = if data.source_artwork.is_some() {
        data.provider_artwork.clone().into_iter().collect()
    } else {
        Vec::new()
    };

    Ok(GroomingPlan {
        source_label: data.inspection.source_label.clone(),
        metadata,
        match_selection,
        match_reasons: reasons,
        destination,
        tracks,
        artwork,
        artwork_alternatives,
        warnings: data.warning.clone().into_iter().collect(),
        preserved_embedded_artwork: data.embedded_artwork_count,
    })
}

fn changes_for(
    source: &InspectedTrack,
    release: &CandidateRelease,
    track: &ReleaseTrack,
) -> Vec<TagChange> {
    let original_year = source.original_year.map(|year| year.to_string());
    let disc_number = source.position.map(|position| position.disc.to_string());
    let track_number = source.position.map(|position| position.track.to_string());
    let proposed = vec![
        (
            TagField::Artist,
            source.artist.clone(),
            track.artist_credit.display.clone(),
        ),
        (
            TagField::AlbumArtist,
            source.album_artist.clone(),
            release.album_artist.display.clone(),
        ),
        (TagField::Album, source.album.clone(), release.title.clone()),
        (
            TagField::OriginalYear,
            original_year,
            release.original_year.to_string(),
        ),
        (
            TagField::DiscNumber,
            disc_number,
            track.position.disc.to_string(),
        ),
        (
            TagField::TrackNumber,
            track_number,
            track.position.track.to_string(),
        ),
        (TagField::Title, source.title.clone(), track.title.clone()),
    ];

    let mut changes: Vec<_> = proposed
        .into_iter()
        .filter_map(|(field, before, after)| {
            (before.as_deref() != Some(after.as_str())).then_some(TagChange {
                field,
                before,
                after,
            })
        })
        .collect();

    if source.recording_id != track.recording_id {
        if let Some(recording_id) = &track.recording_id {
            changes.push(TagChange {
                field: TagField::MusicBrainzRecordingId,
                before: source.recording_id.clone(),
                after: recording_id.clone(),
            });
        }
    }
    if source.release_group_id != release.release_group_id {
        if let Some(release_group_id) = &release.release_group_id {
            changes.push(TagChange {
                field: TagField::MusicBrainzReleaseGroupId,
                before: source.release_group_id.clone(),
                after: release_group_id.clone(),
            });
        }
    }

    changes
}

fn show_summary(interaction: &mut impl Interaction, plan: &GroomingPlan) -> Result<(), DemoError> {
    interaction.show("Preview")?;
    match &plan.metadata {
        MetadataBasis::MusicBrainz(release) => {
            interaction.show(&format!("  Metadata: {}", release.human_label()))?;
            match plan.match_selection {
                MatchSelection::Automatic => {
                    if let Some(reason) = plan.match_reasons.first() {
                        interaction.show(&format!("  Why automatic: {reason}"))?;
                    }
                }
                MatchSelection::UserChosen => {
                    interaction.show("  Decision: selected by you from the plausible matches")?;
                }
                MatchSelection::ExistingTags => {}
            }
        }
        MetadataBasis::ExistingTags => {
            interaction.show("  Metadata: existing tags (not verified against MusicBrainz)")?;
        }
    }
    interaction.show(&format!("  Destination: {}", plan.destination.display()))?;
    interaction.show(&format!("  Artwork: {}", plan.artwork.description()))?;
    interaction.show(&format!(
        "  Changes: {} tag value(s), {} filename(s)",
        plan.tag_change_count(),
        plan.filename_change_count()
    ))?;
    interaction.show(&format!(
        "  Preserved: embedded artwork in {} track(s)",
        plan.preserved_embedded_artwork
    ))?;
    for warning in &plan.warnings {
        interaction.show(&format!("  Warning: {}", warning.summary))?;
    }
    interaction.show("")?;
    Ok(())
}

fn show_details(interaction: &mut impl Interaction, plan: &GroomingPlan) -> Result<(), DemoError> {
    interaction.show("")?;
    interaction.show("All planned changes")?;
    for track in &plan.tracks {
        interaction.show(&format!("  {}", track.source_name))?;
        interaction.show(&format!("    → {}", track.destination.display()))?;
        if track.tag_changes.is_empty() {
            interaction.show("    tags unchanged")?;
        } else {
            for change in &track.tag_changes {
                interaction.show(&format!(
                    "    {}: {} → {}",
                    change.field,
                    change.before.as_deref().unwrap_or("(missing)"),
                    change.after
                ))?;
            }
        }
        interaction.show("    embedded artwork: preserved unchanged")?;
    }
    if let Some(output_name) = &plan.artwork.output_name {
        interaction.show(&format!(
            "  Sidecar artwork: {} → {output_name}",
            plan.artwork.label
        ))?;
    }
    for warning in &plan.warnings {
        interaction.show(&format!(
            "  Warning: {} — {}",
            warning.summary, warning.detail
        ))?;
    }
    interaction.show("")?;
    Ok(())
}

fn choose_artwork(
    interaction: &mut impl Interaction,
    plan: GroomingPlan,
) -> Result<GroomingPlan, DemoError> {
    let mut choices = vec![plan.artwork.clone()];
    choices.extend(plan.artwork_alternatives.clone());
    if choices.len() == 1 {
        interaction.show("No alternative artwork is available for this preview.")?;
        return Ok(plan);
    }

    interaction.show("")?;
    interaction.show("Artwork choices")?;
    for (index, artwork) in choices.iter().enumerate() {
        let selected = if artwork == &plan.artwork {
            " (selected)"
        } else {
            ""
        };
        interaction.show(&format!(
            "  {}. {}{}",
            index + 1,
            artwork.description(),
            selected
        ))?;
    }
    interaction.show("  v. View a choice (simulated in this demo)")?;
    loop {
        let answer = interaction.ask("Artwork number, v to view, or b to go back: ")?;
        match answer.to_ascii_lowercase().as_str() {
            "b" | "back" | "" => return Ok(plan),
            "v" | "view" => {
                let number = interaction.ask("View which artwork number? ")?;
                if let Ok(index) = number.parse::<usize>() {
                    if let Some(artwork) = choices.get(index.saturating_sub(1)) {
                        interaction.show(&format!(
                            "Would open {} in the normal image viewer.",
                            artwork.description()
                        ))?;
                        continue;
                    }
                }
                interaction.show("Please choose one of the displayed artwork numbers.")?;
            }
            _ => {
                if let Ok(index) = answer.parse::<usize>() {
                    if let Some(artwork) = choices.get(index.saturating_sub(1)) {
                        interaction.show(&format!("Selected: {}", artwork.description()))?;
                        interaction.show("")?;
                        return Ok(plan.with_artwork(artwork.clone()));
                    }
                }
                interaction.show("Please choose an artwork number, v, or b.")?;
            }
        }
    }
}

fn no_artwork() -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::None,
        label: "No sidecar artwork".into(),
        dimensions: None,
        output_name: None,
    }
}

fn demo_data(scenario: DemoScenario) -> DemoData {
    match scenario {
        DemoScenario::ConfidentAlbum => confident_album(),
        DemoScenario::AmbiguousCollaboration => ambiguous_collaboration(),
        DemoScenario::MatchedSingle => matched_single(),
        DemoScenario::StandaloneTrack => standalone_track(),
    }
}

fn confident_album() -> DemoData {
    let inspection = Inspection {
        source_label: "demo/incoming/The Group - The Album".into(),
        kind: SourceKind::AlbumDirectory,
        tracks: vec![
            inspected("01 opening.flac", "Opening (old title)", 1, 180_000),
            inspected("02 closing.flac", "Closing", 2, 240_000),
        ],
    };
    let candidate = album_candidate("clear", "The Album", 1971, "The Group", [180_500, 239_500]);
    DemoData {
        inspection,
        candidates: vec![candidate],
        extensions: vec!["flac".into(), "flac".into()],
        source_artwork: Some(source_artwork("folder.jpg", 1800, 1800)),
        provider_artwork: Some(caa_artwork("group-clear")),
        warning: Some(PlanWarning {
            summary: "playlist.m3u may refer to the old filenames".into(),
            detail: "It will be copied unchanged; playlist rewriting is intentionally deferred."
                .into(),
        }),
        embedded_artwork_count: 2,
    }
}

fn ambiguous_collaboration() -> DemoData {
    let credit = "Niels-Henning Ørsted Pedersen & Kenny Drew";
    let mut first = album_candidate("duo", "Duo", 1973, credit, [205_000, 198_000]);
    first.kind = ReleaseKind::Album;
    let second = album_candidate(
        "duo-session",
        "Duo: Studio Session",
        1974,
        credit,
        [205_000, 198_000],
    );
    let mut inspection = Inspection {
        source_label: "demo/incoming/NHOP with Kenny Drew".into(),
        kind: SourceKind::AlbumDirectory,
        tracks: vec![
            inspected("track1.flac", "Opening", 1, 205_000),
            inspected("track2.flac", "Closing", 2, 198_000),
        ],
    };
    for track in &mut inspection.tracks {
        track.album = None;
        track.album_artist = None;
        track.original_year = None;
        track.artist = Some(credit.into());
    }
    DemoData {
        inspection,
        candidates: vec![first, second],
        extensions: vec!["flac".into(), "flac".into()],
        source_artwork: Some(source_artwork("cover.jpg", 2400, 2400)),
        provider_artwork: Some(caa_artwork("group-duo")),
        warning: None,
        embedded_artwork_count: 2,
    }
}

fn matched_single() -> DemoData {
    let inspection = Inspection {
        source_label: "demo/incoming/car-song.opus".into(),
        kind: SourceKind::LooseFile,
        tracks: vec![InspectedTrack {
            source_name: "car-song.opus".into(),
            title: Some("Car Song".into()),
            artist: Some("The Driver".into()),
            album: None,
            album_artist: None,
            original_year: None,
            position: None,
            duration_ms: 201_000,
            recording_id: Some("recording-car-song".into()),
            release_group_id: None,
        }],
    };
    let candidate = CandidateRelease {
        provider_key: "car-single".into(),
        title: "Car Song".into(),
        album_artist: ArtistCredit::single("The Driver"),
        original_year: 2024,
        kind: ReleaseKind::Single,
        tracks: vec![ReleaseTrack {
            title: "Car Song".into(),
            artist_credit: ArtistCredit::single("The Driver"),
            position: Position::new(1, 1),
            duration_ms: 200_500,
            recording_id: Some("recording-car-song".into()),
        }],
        release_group_id: Some("group-car-song".into()),
        exact_release_id: None,
    };
    DemoData {
        inspection,
        candidates: vec![candidate],
        extensions: vec!["opus".into()],
        source_artwork: None,
        provider_artwork: Some(caa_artwork("group-car-song")),
        warning: None,
        embedded_artwork_count: 1,
    }
}

fn standalone_track() -> DemoData {
    DemoData {
        inspection: Inspection {
            source_label: "demo/incoming/mystery-song.mp3".into(),
            kind: SourceKind::LooseFile,
            tracks: vec![InspectedTrack {
                source_name: "mystery-song.mp3".into(),
                title: Some("Mystery Song".into()),
                artist: Some("Bedroom Artist".into()),
                album: None,
                album_artist: None,
                original_year: None,
                position: None,
                duration_ms: 189_000,
                recording_id: None,
                release_group_id: None,
            }],
        },
        candidates: Vec::new(),
        extensions: vec!["mp3".into()],
        source_artwork: None,
        provider_artwork: None,
        warning: Some(PlanWarning {
            summary: "metadata is not verified against MusicBrainz".into(),
            detail: "A later retry can search providers again without changing the source.".into(),
        }),
        embedded_artwork_count: 1,
    }
}

fn inspected(name: &str, title: &str, track: u16, duration_ms: u64) -> InspectedTrack {
    InspectedTrack {
        source_name: name.into(),
        title: Some(title.into()),
        artist: Some("The Group".into()),
        album: Some("The Album".into()),
        album_artist: Some("The Group".into()),
        original_year: Some(1971),
        position: Some(Position::new(1, track)),
        duration_ms,
        recording_id: None,
        release_group_id: None,
    }
}

fn album_candidate(
    key: &str,
    title: &str,
    year: u16,
    artist: &str,
    durations: [u64; 2],
) -> CandidateRelease {
    let credit = if artist.contains(" & ") {
        ArtistCredit::credited(artist, artist.split(" & ").map(Artist::named).collect())
    } else {
        ArtistCredit::single(artist)
    };
    CandidateRelease {
        provider_key: key.into(),
        title: title.into(),
        album_artist: credit.clone(),
        original_year: year,
        kind: ReleaseKind::Album,
        tracks: ["Opening", "Closing"]
            .into_iter()
            .zip(durations)
            .enumerate()
            .map(|(index, (track_title, duration_ms))| ReleaseTrack {
                title: track_title.into(),
                artist_credit: credit.clone(),
                position: Position::new(1, index as u16 + 1),
                duration_ms,
                recording_id: Some(format!("recording-{key}-{}", index + 1)),
            })
            .collect(),
        release_group_id: Some(format!("group-{key}")),
        exact_release_id: None,
    }
}

fn source_artwork(name: &str, width: u32, height: u32) -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::SourceSidecar {
            source_name: name.into(),
        },
        label: format!("existing source {name}"),
        dimensions: Some((width, height)),
        output_name: Some("cover.jpg".into()),
    }
}

fn caa_artwork(release_group_id: &str) -> ArtworkChoice {
    ArtworkChoice {
        origin: ArtworkOrigin::CoverArtArchive {
            release_group_id: release_group_id.into(),
        },
        label: "Cover Art Archive front image".into(),
        dimensions: Some((1200, 1200)),
        output_name: Some("cover.jpg".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedInteraction {
        answers: VecDeque<String>,
        transcript: String,
    }

    impl ScriptedInteraction {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|answer| (*answer).into()).collect(),
                transcript: String::new(),
            }
        }
    }

    impl Interaction for ScriptedInteraction {
        fn show(&mut self, text: &str) -> io::Result<()> {
            self.transcript.push_str(text);
            self.transcript.push('\n');
            Ok(())
        }

        fn ask(&mut self, prompt: &str) -> io::Result<String> {
            self.transcript.push_str(prompt);
            self.answers
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "script ended"))
        }
    }

    #[test]
    fn confident_match_reaches_apply_without_match_question() {
        let mut interaction = ScriptedInteraction::new(&["a", "y"]);

        let outcome = run(
            &mut interaction,
            Some(DemoScenario::ConfidentAlbum),
            Some(Path::new("/chosen-output")),
        )
        .expect("demo should finish");

        assert!(matches!(outcome, DemoOutcome::Applied(_)));
        assert!(interaction.transcript.contains("Matched automatically"));
        assert!(!interaction.transcript.contains("Which looks right?"));
        assert!(
            interaction
                .transcript
                .contains("/chosen-output/The Group/1971 - The Album")
        );
        assert!(interaction.transcript.contains("No files were written"));
    }

    #[test]
    fn ambiguity_is_resolved_in_the_same_session_with_human_labels() {
        let mut interaction = ScriptedInteraction::new(&["2", "a", "yes"]);

        let outcome = run(
            &mut interaction,
            Some(DemoScenario::AmbiguousCollaboration),
            None,
        )
        .expect("demo should finish");

        assert!(matches!(outcome, DemoOutcome::Applied(_)));
        assert!(interaction.transcript.contains("Which looks right?"));
        assert!(
            interaction
                .transcript
                .contains("Niels-Henning Ørsted Pedersen & Kenny Drew — Duo: Studio Session (1974")
        );
        assert!(!interaction.transcript.contains("duo-session"));
    }

    #[test]
    fn review_and_artwork_change_return_to_the_same_preview() {
        let mut interaction = ScriptedInteraction::new(&["r", "w", "v", "2", "2", "a", "y"]);

        let outcome = run(&mut interaction, Some(DemoScenario::ConfidentAlbum), None)
            .expect("demo should finish");

        assert!(matches!(outcome, DemoOutcome::Applied(_)));
        assert!(interaction.transcript.contains("All planned changes"));
        assert!(
            interaction
                .transcript
                .contains("Would open Cover Art Archive")
        );
        assert!(
            interaction
                .transcript
                .contains("Selected: Cover Art Archive")
        );
    }

    #[test]
    fn unmatched_loose_track_is_visibly_unverified_and_can_be_cancelled() {
        let mut interaction = ScriptedInteraction::new(&["c"]);

        let outcome = run(&mut interaction, Some(DemoScenario::StandaloneTrack), None)
            .expect("demo should finish");

        assert_eq!(outcome, DemoOutcome::Cancelled);
        assert!(
            interaction
                .transcript
                .contains("No matching single was found")
        );
        assert!(
            interaction
                .transcript
                .contains("not verified against MusicBrainz")
        );
        assert!(
            interaction
                .transcript
                .contains("Standalone Tracks/Mystery Song")
        );
    }

    #[test]
    fn declining_final_confirmation_returns_to_preview() {
        let mut interaction = ScriptedInteraction::new(&["a", "n", "c"]);

        let outcome = run(&mut interaction, Some(DemoScenario::MatchedSingle), None)
            .expect("demo should finish");

        assert_eq!(outcome, DemoOutcome::Cancelled);
        assert!(
            interaction
                .transcript
                .contains("Apply not confirmed; returning to the preview")
        );
        assert!(
            interaction
                .transcript
                .contains("Artwork: Cover Art Archive front image (1200x1200)")
        );
    }
}
