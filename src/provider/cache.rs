use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AcoustIdResponse, DEFAULT_CACHE_MAX_BYTES, METADATA_FRESH_DAYS, ProviderSearch};
use super::{ProviderArtwork, cover_art_archive};
use crate::domain::CandidateRelease;
use crate::fingerprint::AudioFingerprint;

const CACHE_SCHEMA: u8 = 3;
const MARKER: &str = ".music-groomer-cache";
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct ProviderCache {
    root: PathBuf,
    max_bytes: u64,
}

impl ProviderCache {
    pub fn new(root: PathBuf, max_bytes: u64) -> Self {
        Self { root, max_bytes }
    }

    pub fn platform_default(max_bytes: Option<u64>) -> Result<Self, CacheError> {
        let directories = directories::ProjectDirs::from("", "", "music-groomer")
            .ok_or(CacheError::NoPlatformCacheDirectory)?;
        Ok(Self::new(
            directories.cache_dir().to_owned(),
            max_bytes.unwrap_or(DEFAULT_CACHE_MAX_BYTES),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn metadata(
        &self,
        search: &ProviderSearch,
        now: SystemTime,
    ) -> Result<Option<MetadataCacheEntry>, CacheError> {
        let path = self.metadata_path(search)?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CacheError::Io(path, error)),
        };
        let mut stored: StoredMetadata = serde_json::from_slice(&bytes)
            .map_err(|error| CacheError::Damaged(path.clone(), error.to_string()))?;
        if stored.schema != CACHE_SCHEMA {
            return Err(CacheError::Obsolete {
                path,
                found_schema: stored.schema,
                current_schema: CACHE_SCHEMA,
            });
        }
        let fetched_at = UNIX_EPOCH + Duration::from_secs(stored.fetched_at);
        let age = now.duration_since(fetched_at).unwrap_or_default();
        let freshness = if age <= Duration::from_secs(METADATA_FRESH_DAYS * 24 * 60 * 60) {
            MetadataFreshness::Fresh
        } else {
            MetadataFreshness::Stale
        };
        stored.accessed_at = unix_seconds(now);
        let maintenance_warning = self
            .write_json_atomic(&path, &stored)
            .and_then(|()| self.prune())
            .err()
            .map(|error| error.to_string());
        Ok(Some(MetadataCacheEntry {
            candidates: stored.candidates,
            fetched_at,
            freshness,
            maintenance_warning,
        }))
    }

    pub fn store_metadata(
        &self,
        search: &ProviderSearch,
        candidates: &[CandidateRelease],
        now: SystemTime,
    ) -> Result<(), CacheError> {
        self.ensure_owned_root()?;
        let path = self.metadata_path(search)?;
        let timestamp = unix_seconds(now);
        let stored = StoredMetadata {
            schema: CACHE_SCHEMA,
            fetched_at: timestamp,
            accessed_at: timestamp,
            candidates: candidates.to_vec(),
        };
        self.write_json_atomic(&path, &stored)?;
        self.prune()?;
        Ok(())
    }

    pub fn acoustid(
        &self,
        fingerprint: &AudioFingerprint,
        now: SystemTime,
    ) -> Result<Option<AcoustIdCacheEntry>, CacheError> {
        let path = self.acoustid_path(fingerprint);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CacheError::Io(path, error)),
        };
        let mut stored: StoredAcoustId = serde_json::from_slice(&bytes)
            .map_err(|error| CacheError::Damaged(path.clone(), error.to_string()))?;
        if stored.schema != CACHE_SCHEMA {
            return Err(CacheError::Obsolete {
                path,
                found_schema: stored.schema,
                current_schema: CACHE_SCHEMA,
            });
        }
        let fetched_at = UNIX_EPOCH + Duration::from_secs(stored.fetched_at);
        stored.accessed_at = unix_seconds(now);
        let maintenance_warning = self
            .write_json_atomic(&path, &stored)
            .and_then(|()| self.prune())
            .err()
            .map(|error| error.to_string());
        Ok(Some(AcoustIdCacheEntry {
            response: stored.response,
            fetched_at,
            freshness: freshness(fetched_at, now),
            maintenance_warning,
        }))
    }

    pub fn store_acoustid(
        &self,
        fingerprint: &AudioFingerprint,
        response: &AcoustIdResponse,
        now: SystemTime,
    ) -> Result<(), CacheError> {
        self.ensure_owned_root()?;
        let timestamp = unix_seconds(now);
        self.write_json_atomic(
            &self.acoustid_path(fingerprint),
            &StoredAcoustId {
                schema: CACHE_SCHEMA,
                fetched_at: timestamp,
                accessed_at: timestamp,
                response: response.clone(),
            },
        )?;
        self.prune()
    }

    pub fn artwork(
        &self,
        release_group_id: &str,
        now: SystemTime,
    ) -> Result<Option<ArtworkCacheEntry>, CacheError> {
        let prefix = format!("{}.", digest(release_group_id.as_bytes()));
        let Some(path) = regular_files(&self.root.join("artwork"))?
            .into_iter()
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
        else {
            return Ok(None);
        };
        let bytes = fs::read(&path).map_err(|error| CacheError::Io(path.clone(), error))?;
        if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            let stored: StoredArtworkAbsence = serde_json::from_slice(&bytes)
                .map_err(|error| CacheError::Damaged(path.clone(), error.to_string()))?;
            if stored.schema != CACHE_SCHEMA {
                return Err(CacheError::Obsolete {
                    path,
                    found_schema: stored.schema,
                    current_schema: CACHE_SCHEMA,
                });
            }
            let fetched_at = UNIX_EPOCH + Duration::from_secs(stored.fetched_at);
            let freshness = freshness(fetched_at, now);
            return Ok(Some(ArtworkCacheEntry::ConfirmedAbsent { freshness }));
        }
        let artwork = cover_art_archive::decode(bytes)
            .map_err(|error| CacheError::Damaged(path.clone(), error.to_string()))?;
        Ok(Some(ArtworkCacheEntry::Image(artwork)))
    }

    pub fn store_artwork(
        &self,
        release_group_id: &str,
        artwork: &ProviderArtwork,
    ) -> Result<(), CacheError> {
        self.ensure_owned_root()?;
        let extension = artwork.format.canonical_extension();
        let path = self.root.join("artwork").join(format!(
            "{}.{}",
            digest(release_group_id.as_bytes()),
            extension
        ));
        self.write_bytes_atomic(&path, &artwork.bytes)?;
        let prefix = format!("{}.", digest(release_group_id.as_bytes()));
        for previous in regular_files(&self.root.join("artwork"))? {
            let same_key = previous
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix));
            if same_key && previous != path {
                fs::remove_file(&previous)
                    .map_err(|error| CacheError::Io(previous.clone(), error))?;
            }
        }
        self.prune()
    }

    pub fn store_artwork_absence(
        &self,
        release_group_id: &str,
        now: SystemTime,
    ) -> Result<(), CacheError> {
        self.ensure_owned_root()?;
        let path = self
            .root
            .join("artwork")
            .join(format!("{}.none.json", digest(release_group_id.as_bytes())));
        self.write_json_atomic(
            &path,
            &StoredArtworkAbsence {
                schema: CACHE_SCHEMA,
                fetched_at: unix_seconds(now),
            },
        )?;
        self.remove_other_artwork_entries(release_group_id, &path)?;
        self.prune()
    }

    pub fn status(&self, now: SystemTime) -> Result<CacheStatus, CacheError> {
        let mut status = CacheStatus {
            location: self.root.clone(),
            max_bytes: self.max_bytes,
            ..CacheStatus::default()
        };
        let metadata = self.root.join("metadata");
        for path in regular_files(&metadata)? {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    status.damaged_entries += 1;
                    continue;
                }
            };
            status.total_bytes = status.total_bytes.saturating_add(bytes.len() as u64);
            match serde_json::from_slice::<StoredMetadata>(&bytes) {
                Ok(entry) => {
                    if entry.schema == CACHE_SCHEMA {
                        let fetched = UNIX_EPOCH + Duration::from_secs(entry.fetched_at);
                        if now.duration_since(fetched).unwrap_or_default()
                            <= Duration::from_secs(METADATA_FRESH_DAYS * 24 * 60 * 60)
                        {
                            status.fresh_metadata += 1;
                        } else {
                            status.stale_metadata += 1;
                        }
                    } else {
                        status.obsolete_entries += 1;
                    }
                }
                Err(_) => status.damaged_entries += 1,
            }
        }
        for path in regular_files(&self.root.join("artwork"))? {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    status.damaged_entries += 1;
                    continue;
                }
            };
            status.total_bytes = status.total_bytes.saturating_add(bytes.len() as u64);
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                match serde_json::from_slice::<StoredArtworkAbsence>(&bytes) {
                    Ok(entry) => {
                        if entry.schema == CACHE_SCHEMA {
                            status.confirmed_artwork_absences += 1;
                        } else {
                            status.obsolete_entries += 1;
                        }
                    }
                    Err(_) => status.damaged_entries += 1,
                }
            } else if cover_art_archive::decode(bytes.clone()).is_ok() {
                status.artwork_entries += 1;
                status.artwork_bytes = status.artwork_bytes.saturating_add(bytes.len() as u64);
            } else {
                status.damaged_entries += 1;
            }
        }
        for path in regular_files(&self.root.join("acoustid"))? {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    status.damaged_entries += 1;
                    continue;
                }
            };
            status.total_bytes = status.total_bytes.saturating_add(bytes.len() as u64);
            status.acoustid_bytes = status.acoustid_bytes.saturating_add(bytes.len() as u64);
            match serde_json::from_slice::<StoredAcoustId>(&bytes) {
                Ok(entry) if entry.schema == CACHE_SCHEMA => {
                    if !entry.response.has_usable_recording_associations() {
                        status.acoustid_no_matches += 1;
                    } else if freshness(UNIX_EPOCH + Duration::from_secs(entry.fetched_at), now)
                        == MetadataFreshness::Fresh
                    {
                        status.fresh_acoustid += 1;
                    } else {
                        status.stale_acoustid += 1;
                    }
                }
                Ok(_) => status.obsolete_entries += 1,
                Err(_) => status.damaged_entries += 1,
            }
        }
        Ok(status)
    }

    pub fn clear(&self) -> Result<(), CacheError> {
        if !self.root.exists() {
            return Ok(());
        }
        let marker = self.root.join(MARKER);
        if !marker.is_file() {
            let mut entries = fs::read_dir(&self.root)
                .map_err(|error| CacheError::Io(self.root.clone(), error))?;
            if entries.next().is_none() {
                return fs::remove_dir(&self.root)
                    .map_err(|error| CacheError::Io(self.root.clone(), error));
            }
            return Err(CacheError::NotOwned(self.root.clone()));
        }
        fs::remove_dir_all(&self.root).map_err(|error| CacheError::Io(self.root.clone(), error))
    }

    fn metadata_path(&self, search: &ProviderSearch) -> Result<PathBuf, CacheError> {
        let encoded =
            serde_json::to_vec(search).map_err(|error| CacheError::Serialize(error.to_string()))?;
        Ok(self
            .root
            .join("metadata")
            .join(format!("{}.json", digest(&encoded))))
    }

    fn acoustid_path(&self, fingerprint: &AudioFingerprint) -> PathBuf {
        let identity = format!("{}\n{}", fingerprint.duration_seconds, fingerprint.value);
        self.root
            .join("acoustid")
            .join(format!("{}.json", digest(identity.as_bytes())))
    }

    fn ensure_owned_root(&self) -> Result<(), CacheError> {
        fs::create_dir_all(&self.root).map_err(|error| CacheError::Io(self.root.clone(), error))?;
        let marker = self.root.join(MARKER);
        if marker.exists() && !marker.is_file() {
            return Err(CacheError::NotOwned(self.root.clone()));
        }
        if !marker.exists() {
            let mut entries = fs::read_dir(&self.root)
                .map_err(|error| CacheError::Io(self.root.clone(), error))?;
            if entries.next().is_some() {
                return Err(CacheError::NotOwned(self.root.clone()));
            }
            fs::write(&marker, "music-groomer provider cache\n")
                .map_err(|error| CacheError::Io(marker, error))?;
        }
        Ok(())
    }

    fn remove_other_artwork_entries(
        &self,
        release_group_id: &str,
        keep: &Path,
    ) -> Result<(), CacheError> {
        let prefix = format!("{}.", digest(release_group_id.as_bytes()));
        for previous in regular_files(&self.root.join("artwork"))? {
            let same_key = previous
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix));
            if same_key && previous != keep {
                fs::remove_file(&previous)
                    .map_err(|error| CacheError::Io(previous.clone(), error))?;
            }
        }
        Ok(())
    }

    fn write_json_atomic<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), CacheError> {
        let parent = path
            .parent()
            .ok_or_else(|| CacheError::Serialize("cache path has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|error| CacheError::Io(parent.to_owned(), error))?;
        let temporary = temporary_path(parent);
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            serde_json::to_writer(&mut file, value)
                .map_err(|error| CacheError::Serialize(error.to_string()))?;
            file.flush()
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            file.sync_all()
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            fs::rename(&temporary, path).map_err(|error| CacheError::Io(path.to_owned(), error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn write_bytes_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
        let parent = path
            .parent()
            .ok_or_else(|| CacheError::Serialize("cache path has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|error| CacheError::Io(parent.to_owned(), error))?;
        let temporary = temporary_path(parent);
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            file.write_all(bytes)
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            file.sync_all()
                .map_err(|error| CacheError::Io(temporary.clone(), error))?;
            fs::rename(&temporary, path).map_err(|error| CacheError::Io(path.to_owned(), error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn prune(&self) -> Result<(), CacheError> {
        let mut entries = Vec::new();
        for path in regular_files(&self.root.join("metadata"))? {
            let bytes = fs::read(&path).map_err(|error| CacheError::Io(path.clone(), error))?;
            let accessed = serde_json::from_slice::<StoredMetadata>(&bytes)
                .map_or(0, |entry| entry.accessed_at);
            entries.push((path, bytes.len() as u64, accessed));
        }
        for path in regular_files(&self.root.join("artwork"))? {
            let metadata =
                fs::metadata(&path).map_err(|error| CacheError::Io(path.clone(), error))?;
            let accessed = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .map(unix_seconds)
                .map_err(|error| CacheError::Io(path.clone(), error))?;
            entries.push((path, metadata.len(), accessed));
        }
        for path in regular_files(&self.root.join("acoustid"))? {
            let bytes = fs::read(&path).map_err(|error| CacheError::Io(path.clone(), error))?;
            let accessed = serde_json::from_slice::<StoredAcoustId>(&bytes)
                .map_or(0, |entry| entry.accessed_at);
            entries.push((path, bytes.len() as u64, accessed));
        }
        let mut total = entries.iter().map(|(_, size, _)| size).sum::<u64>();
        entries.sort_by_key(|(_, _, accessed)| *accessed);
        for (path, size, _) in entries {
            if total <= self.max_bytes {
                break;
            }
            fs::remove_file(&path).map_err(|error| CacheError::Io(path, error))?;
            total = total.saturating_sub(size);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataCacheEntry {
    pub candidates: Vec<CandidateRelease>,
    pub fetched_at: SystemTime,
    pub freshness: MetadataFreshness,
    pub maintenance_warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcoustIdCacheEntry {
    pub response: AcoustIdResponse,
    pub fetched_at: SystemTime,
    pub freshness: MetadataFreshness,
    pub maintenance_warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkCacheEntry {
    Image(ProviderArtwork),
    ConfirmedAbsent { freshness: MetadataFreshness },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataFreshness {
    Fresh,
    Stale,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStatus {
    pub location: PathBuf,
    pub total_bytes: u64,
    pub max_bytes: u64,
    pub fresh_metadata: usize,
    pub stale_metadata: usize,
    pub artwork_entries: usize,
    pub artwork_bytes: u64,
    pub confirmed_artwork_absences: usize,
    pub fresh_acoustid: usize,
    pub stale_acoustid: usize,
    pub acoustid_no_matches: usize,
    pub acoustid_bytes: u64,
    pub obsolete_entries: usize,
    pub damaged_entries: usize,
}

#[derive(Debug)]
pub enum CacheError {
    NoPlatformCacheDirectory,
    Io(PathBuf, std::io::Error),
    Obsolete {
        path: PathBuf,
        found_schema: u8,
        current_schema: u8,
    },
    Damaged(PathBuf, String),
    NotOwned(PathBuf),
    Serialize(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlatformCacheDirectory => {
                formatter.write_str("this platform has no user cache directory")
            }
            Self::Io(path, error) => write!(formatter, "{}: {error}", path.display()),
            Self::Obsolete {
                path,
                found_schema,
                current_schema,
            } => write!(
                formatter,
                "obsolete cache entry {} uses schema {found_schema}; current schema is {current_schema}",
                path.display()
            ),
            Self::Damaged(path, error) => {
                write!(formatter, "damaged cache entry {}: {error}", path.display())
            }
            Self::NotOwned(path) => write!(
                formatter,
                "refusing to use unmarked non-empty cache directory {}",
                path.display()
            ),
            Self::Serialize(error) => write!(formatter, "cannot encode cache entry: {error}"),
        }
    }
}

impl std::error::Error for CacheError {}

#[derive(Serialize, Deserialize)]
struct StoredMetadata {
    schema: u8,
    fetched_at: u64,
    accessed_at: u64,
    candidates: Vec<CandidateRelease>,
}

#[derive(Serialize, Deserialize)]
struct StoredArtworkAbsence {
    schema: u8,
    fetched_at: u64,
}

#[derive(Serialize, Deserialize)]
struct StoredAcoustId {
    schema: u8,
    fetched_at: u64,
    accessed_at: u64,
    response: AcoustIdResponse,
}

fn freshness(fetched_at: SystemTime, now: SystemTime) -> MetadataFreshness {
    if now.duration_since(fetched_at).unwrap_or_default()
        <= Duration::from_secs(METADATA_FRESH_DAYS * 24 * 60 * 60)
    {
        MetadataFreshness::Fresh
    } else {
        MetadataFreshness::Stale
    }
}

fn regular_files(directory: &Path) -> Result<Vec<PathBuf>, CacheError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CacheError::Io(directory.to_owned(), error)),
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry) if entry.path().is_file() => Some(Ok(entry.path())),
            Ok(_) => None,
            Err(error) => Some(Err(CacheError::Io(directory.to_owned(), error))),
        })
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn temporary_path(parent: &Path) -> PathBuf {
    parent.join(format!(
        ".write-{}-{}",
        std::process::id(),
        WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests;
