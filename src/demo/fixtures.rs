use crate::domain::{
    Artist, ArtistCredit, CandidateRelease, InspectedTrack, Inspection, Position, ReleaseKind,
    ReleaseTrack, SourceKind,
};
use crate::plan::{ArtworkChoice, ArtworkOrigin, PlanWarning};

use super::DemoScenario;

pub(super) struct DemoData {
    pub inspection: Inspection,
    pub candidates: Vec<CandidateRelease>,
    pub extensions: Vec<String>,
    pub source_artwork: Option<ArtworkChoice>,
    pub provider_artwork: Option<ArtworkChoice>,
    pub warning: Option<PlanWarning>,
    pub embedded_artwork_count: usize,
}

pub(super) fn demo_data(scenario: DemoScenario) -> DemoData {
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
    let first = album_candidate("duo", "Duo", 1973, credit, [205_000, 198_000]);
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
            artist_ids: Vec::new(),
            album_artist_ids: Vec::new(),
            compilation: Some(false),
            original_year: None,
            position: None,
            duration_ms: 201_000,
            recording_id: Some("recording-car-song".into()),
            release_group_id: None,
        }],
    };
    let credit = ArtistCredit::credited(
        "The Driver",
        vec![Artist {
            name: "The Driver".into(),
            musicbrainz_id: Some("artist-driver".into()),
        }],
    );
    let candidate = CandidateRelease {
        provider_key: "car-single".into(),
        title: "Car Song".into(),
        album_artist: credit.clone(),
        original_year: Some(2024),
        kind: ReleaseKind::Single,
        tracks: vec![ReleaseTrack {
            title: "Car Song".into(),
            artist_credit: credit,
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
                artist_ids: Vec::new(),
                album_artist_ids: Vec::new(),
                compilation: None,
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

fn album_candidate(
    key: &str,
    title: &str,
    year: u16,
    artist: &str,
    durations: [u64; 2],
) -> CandidateRelease {
    let artists = artist
        .split(" & ")
        .enumerate()
        .map(|(index, name)| Artist {
            name: name.to_owned(),
            musicbrainz_id: Some(format!("artist-{key}-{}", index + 1)),
        })
        .collect();
    let credit = ArtistCredit::credited(artist, artists);
    CandidateRelease {
        provider_key: key.into(),
        title: title.into(),
        album_artist: credit.clone(),
        original_year: Some(year),
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
