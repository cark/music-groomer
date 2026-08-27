use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbaImage};
use tempfile::TempDir;

use super::*;
use crate::domain::{ArtistCredit, ReleaseKind, SourceKind};

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
    assert!(matches!(cache.clear(), Err(CacheError::NotOwned(_))));
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
    let cached = cache.artwork("group").unwrap().unwrap();

    assert_eq!(cached.artwork.format, crate::source::ArtworkFormat::Png);
    assert_eq!(cached.artwork.dimensions, (3, 4));
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

    let cached = cache.artwork("group").unwrap().unwrap();
    assert_eq!(cached.artwork.format, crate::source::ArtworkFormat::Png);
    assert_eq!(cache.status(UNIX_EPOCH).unwrap().artwork_entries, 1);
}

fn search(album: &str) -> ProviderSearch {
    ProviderSearch {
        kind: SourceKind::AlbumDirectory,
        album: Some(album.into()),
        artist: Some("Artist".into()),
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
