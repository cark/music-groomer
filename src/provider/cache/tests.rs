use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbaImage};
use tempfile::TempDir;

use super::*;
use crate::domain::{ArtistCredit, ReleaseKind, SourceKind};
use crate::fingerprint::AudioFingerprint;
use crate::provider::{AcoustIdResponse, AcoustIdResult};

#[test]
fn fresh_and_stale_entries_are_distinguished_with_controlled_time() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let now = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    cache
        .store_metadata(&search("Album"), &[candidate("one")], now)
        .unwrap();

    let fresh = cache.metadata(&search("Album"), now).unwrap().unwrap();
    let stale_time = now + Duration::from_secs((METADATA_FRESH_DAYS + 1) * 86_400);
    let stale = cache
        .metadata(&search("Album"), stale_time)
        .unwrap()
        .unwrap();

    assert_eq!(fresh.freshness, MetadataFreshness::Fresh);
    assert_eq!(stale.freshness, MetadataFreshness::Stale);
    assert_eq!(stale.candidates[0].provider_key, "one");
}

#[test]
fn damaged_entry_costs_only_a_cache_miss_signal() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let search = search("Album");
    cache
        .store_metadata(&search, &[candidate("one")], UNIX_EPOCH)
        .unwrap();
    fs::write(cache.metadata_path(&search).unwrap(), "broken").unwrap();

    assert!(matches!(
        cache.metadata(&search, UNIX_EPOCH),
        Err(CacheError::Damaged(_, _))
    ));
    assert_eq!(cache.status(UNIX_EPOCH).unwrap().damaged_entries, 1);
}

#[test]
fn older_schema_is_obsolete_rather_than_damaged_and_still_counts_toward_usage() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let search = search("Album");
    cache
        .store_metadata(&search, &[candidate("one")], UNIX_EPOCH)
        .unwrap();
    let path = cache.metadata_path(&search).unwrap();
    let mut stored =
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap();
    stored["schema"] = serde_json::json!(CACHE_SCHEMA - 1);
    fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert!(matches!(
        cache.metadata(&search, UNIX_EPOCH),
        Err(CacheError::Obsolete {
            found_schema,
            current_schema,
            ..
        }) if found_schema == CACHE_SCHEMA - 1 && current_schema == CACHE_SCHEMA
    ));
    let status = cache.status(UNIX_EPOCH).unwrap();
    assert_eq!(status.obsolete_entries, 1);
    assert_eq!(status.damaged_entries, 0);
    assert_eq!(status.total_bytes, fs::metadata(path).unwrap().len());
}

#[test]
fn tiny_limit_prunes_least_recently_used_entries_on_write() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("cache");
    let roomy = ProviderCache::new(root.clone(), 1024 * 1024);
    roomy
        .store_metadata(&search("Old"), &[candidate("old")], UNIX_EPOCH)
        .unwrap();
    let old_path = roomy.metadata_path(&search("Old")).unwrap();
    let one_entry_size = fs::metadata(&old_path).unwrap().len();
    let tiny = ProviderCache::new(root, one_entry_size + 8);
    tiny.store_metadata(
        &search("New"),
        &[candidate("new")],
        UNIX_EPOCH + Duration::from_secs(1),
    )
    .unwrap();

    assert!(!old_path.exists());
    assert!(tiny.metadata_path(&search("New")).unwrap().exists());
}

#[test]
fn status_is_read_only_and_clear_requires_ownership_marker() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("cache");
    let cache = ProviderCache::new(root.clone(), 1024);

    assert_eq!(cache.status(UNIX_EPOCH).unwrap().total_bytes, 0);
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    cache
        .clear()
        .expect("an empty override is already safe to clear");
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    fs::write(root.join("unrelated"), "keep me").unwrap();
    assert!(matches!(cache.clear(), Err(CacheError::NotOwned(_))));
}

#[test]
fn writing_refuses_to_claim_a_non_empty_unmarked_directory() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("cache");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("unrelated"), "keep me").unwrap();
    let cache = ProviderCache::new(root.clone(), 1024 * 1024);

    assert!(matches!(
        cache.store_metadata(&search("Album"), &[candidate("one")], UNIX_EPOCH),
        Err(CacheError::NotOwned(_))
    ));
    assert_eq!(
        fs::read_to_string(root.join("unrelated")).unwrap(),
        "keep me"
    );
}

#[test]
fn artwork_round_trips_in_its_native_format() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(RgbaImage::new(3, 4))
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    let artwork = cover_art_archive::decode(bytes).unwrap();

    cache.store_artwork("group", &artwork).unwrap();
    let cached = cache.artwork("group", UNIX_EPOCH).unwrap().unwrap();

    let ArtworkCacheEntry::Image(cached) = cached else {
        panic!("expected cached image");
    };
    assert_eq!(cached.format, crate::source::ArtworkFormat::Png);
    assert_eq!(cached.dimensions, (3, 4));
    assert_eq!(cache.status(UNIX_EPOCH).unwrap().artwork_entries, 1);
}

#[test]
fn replacing_artwork_removes_the_previous_native_format() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let mut jpeg = Vec::new();
    DynamicImage::ImageRgba8(RgbaImage::new(3, 4))
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .unwrap();
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(RgbaImage::new(5, 6))
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();

    cache
        .store_artwork("group", &cover_art_archive::decode(jpeg).unwrap())
        .unwrap();
    cache
        .store_artwork("group", &cover_art_archive::decode(png).unwrap())
        .unwrap();

    let cached = cache.artwork("group", UNIX_EPOCH).unwrap().unwrap();
    let ArtworkCacheEntry::Image(cached) = cached else {
        panic!("expected cached image");
    };
    assert_eq!(cached.format, crate::source::ArtworkFormat::Png);
    assert_eq!(cache.status(UNIX_EPOCH).unwrap().artwork_entries, 1);
}

#[test]
fn confirmed_artwork_absence_is_fresh_for_thirty_days_and_counted_separately() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    cache.store_artwork_absence("group", UNIX_EPOCH).unwrap();

    assert!(matches!(
        cache
            .artwork("group", UNIX_EPOCH + Duration::from_secs(29 * 86_400))
            .unwrap(),
        Some(ArtworkCacheEntry::ConfirmedAbsent {
            freshness: MetadataFreshness::Fresh
        })
    ));
    assert!(matches!(
        cache
            .artwork("group", UNIX_EPOCH + Duration::from_secs(31 * 86_400))
            .unwrap(),
        Some(ArtworkCacheEntry::ConfirmedAbsent {
            freshness: MetadataFreshness::Stale
        })
    ));
    let status = cache.status(UNIX_EPOCH).unwrap();
    assert_eq!(status.artwork_entries, 0);
    assert_eq!(status.confirmed_artwork_absences, 1);
}

#[test]
fn acoustid_matches_and_no_matches_share_status_pruning_and_clear() {
    let temporary = TempDir::new().unwrap();
    let cache = ProviderCache::new(temporary.path().join("cache"), 1024 * 1024);
    let matched = AudioFingerprint {
        duration_seconds: 120,
        value: "matched-fingerprint".into(),
    };
    let no_match = AudioFingerprint {
        duration_seconds: 121,
        value: "no-match-fingerprint".into(),
    };
    cache
        .store_acoustid(
            &matched,
            &AcoustIdResponse {
                results: vec![AcoustIdResult {
                    id: "result".into(),
                    score: 0.95,
                    recording_ids: vec!["recording".into()],
                }],
            },
            UNIX_EPOCH,
        )
        .unwrap();
    cache
        .store_acoustid(&no_match, &AcoustIdResponse::default(), UNIX_EPOCH)
        .unwrap();

    let stored = fs::read_to_string(cache.acoustid_path(&matched)).unwrap();
    assert!(!stored.contains("matched-fingerprint"));

    let status = cache.status(UNIX_EPOCH).unwrap();
    assert_eq!(status.fresh_acoustid, 1);
    assert_eq!(status.acoustid_no_matches, 1);
    assert_eq!(status.stale_acoustid, 0);
    assert!(status.total_bytes > 0);

    cache.clear().unwrap();
    assert!(!cache.root().exists());
}

fn search(album: &str) -> ProviderSearch {
    ProviderSearch {
        kind: SourceKind::AlbumDirectory,
        album: Some(album.into()),
        artist: Some("Artist".into()),
        artist_ids: Vec::new(),
        album_artist_ids: Vec::new(),
        title: None,
        release_group_id: None,
        recording_ids: Vec::new(),
        track_count: 1,
    }
}

fn candidate(key: &str) -> CandidateRelease {
    CandidateRelease {
        provider_key: key.into(),
        title: "Album".into(),
        album_artist: ArtistCredit::single("Artist"),
        original_year: Some(2000),
        kind: ReleaseKind::Album,
        tracks: Vec::new(),
        release_group_id: Some("group".into()),
        exact_release_id: None,
    }
}
