use std::collections::BTreeSet;

use crate::domain::Inspection;
use crate::guided_matching::{ArtworkSelection, MetadataSelection};
use crate::source::{NoticeSeverity, SourceInspection};

#[derive(Clone, Debug)]
pub(super) struct WarningState {
    persistent: Vec<String>,
    metadata: Vec<String>,
    identification: Vec<String>,
    selection: Vec<String>,
    artwork: Vec<String>,
}

impl WarningState {
    pub(super) fn new(source: &SourceInspection, inspection: &Inspection) -> Self {
        let mut persistent = source_warnings(source);
        persistent.extend(identifier_warnings(inspection));
        Self {
            persistent: deduplicated(persistent),
            metadata: Vec::new(),
            identification: Vec::new(),
            selection: Vec::new(),
            artwork: Vec::new(),
        }
    }

    pub(super) fn set_metadata(&mut self, warnings: Vec<String>) {
        self.metadata = warnings;
    }

    pub(super) fn set_identification(&mut self, warnings: Vec<String>) {
        self.identification = warnings;
    }

    pub(super) fn set_selection(&mut self, warnings: Vec<String>) {
        self.selection = warnings;
    }

    pub(super) fn set_artwork(
        &mut self,
        mut warnings: Vec<String>,
        artwork: &ArtworkSelection,
        archive: Option<&crate::provider::ProviderArtwork>,
    ) {
        if artwork == &ArtworkSelection::None && archive.is_none() {
            warnings.push("No album artwork is available".into());
        }
        self.artwork = warnings;
    }

    pub(super) fn current(&self) -> Vec<String> {
        deduplicated(
            self.persistent
                .iter()
                .chain(&self.metadata)
                .chain(&self.identification)
                .chain(&self.selection)
                .chain(&self.artwork)
                .cloned()
                .collect(),
        )
    }
}

pub(super) fn selection_year_warnings(
    inspection: &Inspection,
    metadata: &mut MetadataSelection,
) -> (Option<u16>, Vec<String>) {
    let source_year = inspection.tracks.first().and_then(|track| {
        let year = track.original_year?;
        inspection
            .tracks
            .iter()
            .all(|track| track.original_year == Some(year))
            .then_some(year)
    });
    let MetadataSelection::Provider(selected) = metadata else {
        return (None, Vec::new());
    };
    if selected.candidate.original_year.is_some() {
        return (None, Vec::new());
    }
    if let Some(year) = source_year {
        selected.candidate.original_year = Some(year);
        return (
            Some(year),
            vec![format!(
                "Selected MusicBrainz metadata has no original year; source year {year} is used as an unverified fallback"
            )],
        );
    }
    (
        None,
        vec![
            "MusicBrainz and the source do not provide an original year; it will remain absent"
                .into(),
        ],
    )
}

pub(super) fn identifier_warnings(inspection: &Inspection) -> Vec<String> {
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
        .collect::<BTreeSet<_>>();
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
        .collect::<BTreeSet<_>>();
    if album_artist_ids.len() > 1 {
        warnings.push(
            "Source MusicBrainz album-artist identifiers are inconsistent; using artist names instead"
                .into(),
        );
    }
    warnings
}

fn source_warnings(source: &SourceInspection) -> Vec<String> {
    source
        .notices
        .iter()
        .filter(|notice| notice.severity == NoticeSeverity::Warning)
        .map(|notice| match &notice.path {
            Some(path) => format!("{}: {}", path.display(), notice.message),
            None => notice.message.clone(),
        })
        .collect()
}

fn deduplicated(warnings: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    warnings
        .into_iter()
        .filter(|warning| seen.insert(warning.clone()))
        .collect()
}
