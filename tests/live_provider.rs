use music_groomer::domain::SourceKind;
use music_groomer::fingerprint::{AudioFingerprinter, FpcalcFingerprinter};
use music_groomer::provider::{
    AcoustId, AcoustIdProvider, ArtworkProvider, CoverArtArchive, MetadataProvider,
    MusicBrainzProvider, ProviderSearch,
};

#[test]
#[ignore = "explicit live MusicBrainz smoke test; run with --ignored"]
fn musicbrainz_returns_a_small_real_search_result() {
    let search = ProviderSearch {
        kind: SourceKind::AlbumDirectory,
        album: Some("Evolution".into()),
        artist: Some("Ten Years After".into()),
        artist_ids: Vec::new(),
        album_artist_ids: Vec::new(),
        title: None,
        release_group_id: None,
        recording_ids: Vec::new(),
        track_count: 10,
    };

    let candidates = MusicBrainzProvider::new()
        .search(&search, &mut ())
        .expect("live MusicBrainz query should succeed");

    assert!(
        candidates
            .candidates
            .iter()
            .any(|candidate| candidate.title.eq_ignore_ascii_case("Evolution")),
        "expected an Evolution release candidate"
    );
}

#[test]
#[ignore = "explicit live Cover Art Archive smoke test; run with --ignored"]
fn cover_art_archive_returns_a_decodable_release_group_front() {
    let artwork = CoverArtArchive::new()
        .front("2a1eeaa6-b3df-373e-b2a7-4064a5050cbd", &mut ())
        .expect("live Cover Art Archive query should succeed")
        .expect("Evolution should have front artwork");

    assert!(artwork.dimensions.0 > 0);
    assert!(artwork.dimensions.1 > 0);
}

#[test]
#[ignore = "explicit live AcoustID smoke test using only the synthetic audio fixture"]
fn acoustid_accepts_a_fingerprint_from_the_synthetic_fixture() {
    let temporary = tempfile::TempDir::new().expect("temporary fixture directory should exist");
    let fixture = temporary.path().join("synthetic-tone.flac");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=20",
            "-y",
        ])
        .arg(&fixture)
        .status()
        .expect("ffmpeg should generate the temporary synthetic fixture");
    assert!(generated.success());
    let fingerprint = FpcalcFingerprinter::default()
        .calculate(&fixture, &mut ())
        .expect("fpcalc should fingerprint the synthetic fixture");

    AcoustId::new()
        .lookup(&fingerprint, &mut ())
        .expect("AcoustID should accept a lookup even when the synthetic audio has no match");
}
