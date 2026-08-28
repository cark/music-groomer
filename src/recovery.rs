use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

pub const RECOVERY_DIRECTORY: &str = ".music-groomer-recovery";
pub const ACTIVE_RECEIPT: &str = ".music-groomer";

const MARKER: &str = ".music-groomer-owned";
const MARKER_CONTENTS: &[u8] = b"music-groomer recovery store v1\n";
const NAVIDROME_IGNORE: &str = ".ndignore";
const INDEX: &str = "index.json";
const LOCK: &str = ".lock";
const INDEX_SCHEMA: u8 = 2;
const RECEIPT_SCHEMA: u8 = 1;
const VERSION_MARKER: &str = ".music-groomer-version";
const VERSION_MARKER_SCHEMA: u8 = 1;
const PAYLOADS: &str = "payloads";
const MAX_HUMAN_COMPONENT_CHARS: usize = 64;
const MAX_HUMAN_COMPONENT_UNITS: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryIndex {
    schema: u8,
    pub lineages: Vec<ReleaseLineage>,
}

impl Default for RecoveryIndex {
    fn default() -> Self {
        Self {
            schema: INDEX_SCHEMA,
            lineages: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseLineage {
    pub lineage_id: String,
    pub active_version_id: String,
    pub expected_active_path: PathBuf,
    pub retained_versions: Vec<RetainedVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedVersion {
    pub version_id: String,
    pub historical_path: PathBuf,
    pub display_label: String,
    pub retained_at: u64,
    pub protected_until: u64,
    pub size_bytes: u64,
    pub storage_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveReceipt {
    schema: u8,
    pub lineage_id: String,
    pub version_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionMarker {
    schema: u8,
    lineage_id: String,
    version_id: String,
}

impl ActiveReceipt {
    pub fn new(lineage_id: impl Into<String>, version_id: impl Into<String>) -> Self {
        Self {
            schema: RECEIPT_SCHEMA,
            lineage_id: lineage_id.into(),
            version_id: version_id.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryStore {
    library_root: PathBuf,
    root: PathBuf,
}

#[derive(Debug)]
pub struct RecoveryLock {
    _file: File,
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRetainedPayload {
    pub payload: PathBuf,
    pub storage_path: PathBuf,
}

impl RecoveryStore {
    pub fn create_or_open(library_root: &Path) -> Result<Self, RecoveryError> {
        require_existing_directory(library_root)?;
        let store = Self {
            library_root: library_root.to_owned(),
            root: library_root.join(RECOVERY_DIRECTORY),
        };
        match fs::symlink_metadata(&store.root) {
            Ok(_) => store.verify_ownership()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                store.initialize()?;
            }
            Err(source) => return Err(RecoveryError::Io(store.root.clone(), source)),
        }
        Ok(store)
    }

    pub fn open_existing(library_root: &Path) -> Result<Option<Self>, RecoveryError> {
        require_existing_directory(library_root)?;
        let store = Self {
            library_root: library_root.to_owned(),
            root: library_root.join(RECOVERY_DIRECTORY),
        };
        match fs::symlink_metadata(&store.root) {
            Ok(_) => {
                store.verify_ownership()?;
                Ok(Some(store))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(RecoveryError::Io(store.root.clone(), source)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load_index(&self) -> Result<RecoveryIndex, RecoveryError> {
        self.verify_ownership()?;
        let path = self.root.join(INDEX);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let payload_root = self.root.join(PAYLOADS);
                if fs::symlink_metadata(&payload_root).is_ok() {
                    return Err(RecoveryError::Damaged(
                        path,
                        format!(
                            "recovery index is missing while payload storage exists at {}",
                            payload_root.display()
                        ),
                    ));
                }
                return Ok(RecoveryIndex::default());
            }
            Err(source) => return Err(RecoveryError::Io(path, source)),
        };
        let index: RecoveryIndex = serde_json::from_slice(&bytes)
            .map_err(|error| RecoveryError::Damaged(path.clone(), error.to_string()))?;
        validate_index(&index).map_err(|cause| RecoveryError::Damaged(path, cause))?;
        Ok(index)
    }

    pub fn lock(&self) -> Result<RecoveryLock, RecoveryError> {
        self.verify_ownership()?;
        let path = self.root.join(LOCK);
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|source| RecoveryError::Io(path.clone(), source))?;
                if !metadata.file_type().is_file() {
                    return Err(RecoveryError::NotOwned(path));
                }
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|source| RecoveryError::Io(path.clone(), source))?
            }
            Err(source) => return Err(RecoveryError::Io(path, source)),
        };
        file.lock_exclusive()
            .map_err(|source| RecoveryError::Io(self.root.join(LOCK), source))?;
        Ok(RecoveryLock {
            _file: file,
            root: self.root.clone(),
        })
    }

    pub fn save_index(
        &self,
        lock: &RecoveryLock,
        index: &RecoveryIndex,
    ) -> Result<(), RecoveryError> {
        self.verify_lock(lock)?;
        self.verify_ownership()?;
        validate_index(index)
            .map_err(|cause| RecoveryError::Damaged(self.root.join(INDEX), cause))?;
        write_json_atomic(&self.root.join(INDEX), index)
    }

    pub fn write_active_receipt(
        &self,
        lock: &RecoveryLock,
        active_directory: &Path,
        receipt: &ActiveReceipt,
    ) -> Result<(), RecoveryError> {
        self.verify_lock(lock)?;
        self.verify_active_directory(active_directory)?;
        validate_id(&receipt.lineage_id)?;
        validate_id(&receipt.version_id)?;
        write_json_atomic(&active_directory.join(ACTIVE_RECEIPT), receipt)
    }

    pub fn read_active_receipt(
        &self,
        active_directory: &Path,
    ) -> Result<ActiveReceipt, RecoveryError> {
        self.verify_ownership()?;
        self.verify_active_directory(active_directory)?;
        let path = active_directory.join(ACTIVE_RECEIPT);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|source| RecoveryError::Io(path.clone(), source))?;
        if !metadata.file_type().is_file() {
            return Err(RecoveryError::Damaged(
                path,
                "active receipt is not a regular file".into(),
            ));
        }
        let bytes = fs::read(&path).map_err(|source| RecoveryError::Io(path.clone(), source))?;
        let receipt: ActiveReceipt = serde_json::from_slice(&bytes)
            .map_err(|error| RecoveryError::Damaged(path.clone(), error.to_string()))?;
        if receipt.schema != RECEIPT_SCHEMA {
            return Err(RecoveryError::Damaged(
                path,
                format!("unsupported active receipt schema {}", receipt.schema),
            ));
        }
        validate_id(&receipt.lineage_id)?;
        validate_id(&receipt.version_id)?;
        Ok(receipt)
    }

    pub fn verify_active_lineage(
        &self,
        lineage: &ReleaseLineage,
    ) -> Result<PathBuf, RecoveryError> {
        validate_lineage(lineage)?;
        let active = self.library_root.join(&lineage.expected_active_path);
        let receipt = self.read_active_receipt(&active)?;
        if receipt.lineage_id != lineage.lineage_id
            || receipt.version_id != lineage.active_version_id
        {
            return Err(RecoveryError::IdentityMismatch(active));
        }
        Ok(active)
    }

    pub fn retained_payload_path(&self, storage_path: &Path) -> Result<PathBuf, RecoveryError> {
        validate_storage_path(storage_path)?;
        Ok(self.root.join(storage_path).join("payload"))
    }

    pub fn prepare_retained_payload(
        &self,
        lock: &RecoveryLock,
        lineage_id: &str,
        version_id: &str,
        display_label: &str,
        retained_at: u64,
        existing_lineage_storage: Option<&Path>,
    ) -> Result<PreparedRetainedPayload, RecoveryError> {
        self.verify_lock(lock)?;
        self.verify_ownership()?;
        validate_id(lineage_id)?;
        validate_id(version_id)?;
        let version_label = retained_timestamp_component(retained_at)?;
        let payloads = self.root.join(PAYLOADS);
        ensure_directory_chain(&self.root, &payloads)?;
        let (lineage, allocated_lineage) = match existing_lineage_storage {
            Some(relative) => {
                validate_lineage_storage_path(relative)?;
                let path = self.root.join(relative);
                require_directory(&path)?;
                (path, false)
            }
            None => match allocate_human_directory(&payloads, display_label) {
                Ok(path) => (path, true),
                Err(error) => {
                    let _ = fs::remove_dir(&payloads);
                    return Err(error);
                }
            },
        };
        let container = match allocate_human_directory(&lineage, &version_label) {
            Ok(path) => path,
            Err(error) => {
                if allocated_lineage {
                    let _ = fs::remove_dir(&lineage);
                }
                let _ = fs::remove_dir(&payloads);
                return Err(error);
            }
        };
        let marker = VersionMarker {
            schema: VERSION_MARKER_SCHEMA,
            lineage_id: lineage_id.to_owned(),
            version_id: version_id.to_owned(),
        };
        if let Err(error) = write_json_new(&container.join(VERSION_MARKER), &marker) {
            let _ = fs::remove_dir(&container);
            if allocated_lineage {
                let _ = fs::remove_dir(&lineage);
            }
            let _ = fs::remove_dir(&payloads);
            return Err(error);
        }
        let storage_path = container
            .strip_prefix(&self.root)
            .expect("allocated recovery container is inside its store")
            .to_owned();
        Ok(PreparedRetainedPayload {
            payload: container.join("payload"),
            storage_path,
        })
    }

    pub fn verify_retained_payload(
        &self,
        lineage_id: &str,
        version_id: &str,
        storage_path: &Path,
    ) -> Result<PathBuf, RecoveryError> {
        self.verify_ownership()?;
        validate_id(lineage_id)?;
        validate_id(version_id)?;
        let payload = self.retained_payload_path(storage_path)?;
        let container = payload
            .parent()
            .expect("retained payload always has a container");
        verify_existing_directory_chain(&self.root, container)?;
        let marker_path = container.join(VERSION_MARKER);
        let marker: VersionMarker = read_regular_json(&marker_path)?;
        if marker.schema != VERSION_MARKER_SCHEMA
            || marker.lineage_id != lineage_id
            || marker.version_id != version_id
        {
            return Err(RecoveryError::IdentityMismatch(container.to_owned()));
        }
        require_directory(&payload)?;
        Ok(payload)
    }

    pub fn retained_size(
        &self,
        lineage_id: &str,
        version_id: &str,
        storage_path: &Path,
    ) -> Result<u64, RecoveryError> {
        let payload = self.verify_retained_payload(lineage_id, version_id, storage_path)?;
        strict_tree_size(&payload)
    }

    pub fn discard_prepared_payload(
        &self,
        lock: &RecoveryLock,
        lineage_id: &str,
        version_id: &str,
        storage_path: &Path,
    ) -> Result<(), RecoveryError> {
        self.verify_lock(lock)?;
        self.verify_ownership()?;
        validate_id(lineage_id)?;
        validate_id(version_id)?;
        let payload = self.retained_payload_path(storage_path)?;
        let container = payload
            .parent()
            .expect("retained payload always has a container");
        verify_existing_directory_chain(&self.root, container)?;
        if fs::symlink_metadata(&payload).is_ok() {
            return Err(RecoveryError::UnsafePath(payload));
        }
        let marker_path = container.join(VERSION_MARKER);
        let marker: VersionMarker = read_regular_json(&marker_path)?;
        if marker.schema != VERSION_MARKER_SCHEMA
            || marker.lineage_id != lineage_id
            || marker.version_id != version_id
        {
            return Err(RecoveryError::IdentityMismatch(container.to_owned()));
        }
        fs::remove_file(&marker_path)
            .map_err(|source| RecoveryError::Io(marker_path.clone(), source))?;
        fs::remove_dir(container)
            .map_err(|source| RecoveryError::Io(container.to_owned(), source))?;
        if let Some(lineage) = container.parent() {
            let _ = fs::remove_dir(lineage);
            if let Some(payloads) = lineage.parent() {
                let _ = fs::remove_dir(payloads);
            }
        }
        Ok(())
    }

    fn initialize(&self) -> Result<(), RecoveryError> {
        fs::create_dir(&self.root)
            .map_err(|source| RecoveryError::Io(self.root.clone(), source))?;
        (|| {
            write_new_file(&self.root.join(NAVIDROME_IGNORE), b"")?;
            write_new_file(&self.root.join(MARKER), MARKER_CONTENTS)?;
            self.verify_ownership()
        })()
    }

    fn verify_ownership(&self) -> Result<(), RecoveryError> {
        require_directory(&self.root)?;
        require_exact_regular_file(&self.root.join(MARKER), MARKER_CONTENTS)?;
        require_exact_regular_file(&self.root.join(NAVIDROME_IGNORE), b"")
    }

    fn verify_active_directory(&self, path: &Path) -> Result<(), RecoveryError> {
        require_directory(path)?;
        let canonical_root = self
            .library_root
            .canonicalize()
            .map_err(|source| RecoveryError::Io(self.library_root.clone(), source))?;
        let canonical_path = path
            .canonicalize()
            .map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
        if canonical_path == canonical_root
            || !canonical_path.starts_with(&canonical_root)
            || canonical_path.starts_with(canonical_root.join(RECOVERY_DIRECTORY))
        {
            return Err(RecoveryError::UnsafePath(path.to_owned()));
        }
        Ok(())
    }

    fn verify_lock(&self, lock: &RecoveryLock) -> Result<(), RecoveryError> {
        if lock.root != self.root {
            return Err(RecoveryError::InvalidMetadata(
                "recovery lock belongs to a different store".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaintenancePlan {
    pub usage_before: u64,
    pub usage_after: u64,
    pub evictions: Vec<Eviction>,
    pub earliest_protected_until: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eviction {
    pub lineage_id: String,
    pub version_id: String,
    pub display_label: String,
    pub retained_at: u64,
    pub size_bytes: u64,
}

pub fn plan_maintenance(index: &RecoveryIndex, max_bytes: u64, now: u64) -> MaintenancePlan {
    let usage_before = index
        .lineages
        .iter()
        .flat_map(|lineage| &lineage.retained_versions)
        .map(|version| version.size_bytes)
        .fold(0_u64, u64::saturating_add);
    let mut usage_after = usage_before;
    let mut eligible = Vec::new();
    let mut earliest_protected_until = None;

    for lineage in &index.lineages {
        for version in &lineage.retained_versions {
            if version.protected_until <= now {
                eligible.push((lineage, version));
            } else {
                earliest_protected_until = Some(
                    earliest_protected_until.map_or(version.protected_until, |current: u64| {
                        current.min(version.protected_until)
                    }),
                );
            }
        }
    }
    eligible.sort_by(|(left_lineage, left), (right_lineage, right)| {
        left.retained_at
            .cmp(&right.retained_at)
            .then_with(|| left.version_id.cmp(&right.version_id))
            .then_with(|| left_lineage.lineage_id.cmp(&right_lineage.lineage_id))
    });

    let mut evictions = Vec::new();
    for (lineage, version) in eligible {
        if usage_after <= max_bytes {
            break;
        }
        usage_after = usage_after.saturating_sub(version.size_bytes);
        evictions.push(Eviction {
            lineage_id: lineage.lineage_id.clone(),
            version_id: version.version_id.clone(),
            display_label: version.display_label.clone(),
            retained_at: version.retained_at,
            size_bytes: version.size_bytes,
        });
    }

    MaintenancePlan {
        usage_before,
        usage_after,
        evictions,
        earliest_protected_until: (usage_after > max_bytes)
            .then_some(earliest_protected_until)
            .flatten(),
    }
}

pub fn new_lineage_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn new_version_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn validate_index(index: &RecoveryIndex) -> Result<(), String> {
    if index.schema != INDEX_SCHEMA {
        return Err(format!(
            "unsupported recovery index schema {}",
            index.schema
        ));
    }
    let mut lineage_ids = BTreeSet::new();
    let mut version_ids = BTreeSet::new();
    let mut active_paths = BTreeSet::new();
    let mut storage_paths = BTreeSet::new();
    for lineage in &index.lineages {
        validate_lineage(lineage).map_err(|error| error.to_string())?;
        if !lineage_ids.insert(&lineage.lineage_id) {
            return Err(format!(
                "duplicate lineage identifier {}",
                lineage.lineage_id
            ));
        }
        if !version_ids.insert(&lineage.active_version_id) {
            return Err(format!(
                "duplicate version identifier {}",
                lineage.active_version_id
            ));
        }
        if !active_paths.insert(&lineage.expected_active_path) {
            return Err(format!(
                "duplicate expected active path {}",
                lineage.expected_active_path.display()
            ));
        }
        for version in &lineage.retained_versions {
            if !version_ids.insert(&version.version_id) {
                return Err(format!(
                    "duplicate version identifier {}",
                    version.version_id
                ));
            }
            if !storage_paths.insert(&version.storage_path) {
                return Err(format!(
                    "duplicate recovery storage path {}",
                    version.storage_path.display()
                ));
            }
        }
    }
    Ok(())
}

fn validate_lineage(lineage: &ReleaseLineage) -> Result<(), RecoveryError> {
    validate_id(&lineage.lineage_id)?;
    validate_id(&lineage.active_version_id)?;
    validate_relative_path(&lineage.expected_active_path)?;
    for version in &lineage.retained_versions {
        validate_id(&version.version_id)?;
        validate_relative_path(&version.historical_path)?;
        validate_storage_path(&version.storage_path)?;
        if version.display_label.trim().is_empty() {
            return Err(RecoveryError::InvalidMetadata(
                "retained version display label is empty".into(),
            ));
        }
        if version.protected_until < version.retained_at {
            return Err(RecoveryError::InvalidMetadata(format!(
                "retained version {} is protected until before it was retained",
                version.version_id
            )));
        }
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), RecoveryError> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| {
        RecoveryError::InvalidMetadata(format!("invalid recovery identifier {value:?}"))
    })?;
    if parsed.to_string() != value {
        return Err(RecoveryError::InvalidMetadata(format!(
            "recovery identifier is not canonical: {value:?}"
        )));
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), RecoveryError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RecoveryError::UnsafePath(path.to_owned()));
    }
    if path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == RECOVERY_DIRECTORY)
    {
        return Err(RecoveryError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn validate_lineage_storage_path(path: &Path) -> Result<(), RecoveryError> {
    let components = path.components().collect::<Vec<_>>();
    if components.len() != 2
        || components[0].as_os_str() != PAYLOADS
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RecoveryError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn validate_storage_path(path: &Path) -> Result<(), RecoveryError> {
    let components = path.components().collect::<Vec<_>>();
    if components.len() != 3
        || components[0].as_os_str() != PAYLOADS
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RecoveryError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), RecoveryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| RecoveryError::Io(path.to_owned(), source))
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<(), RecoveryError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| RecoveryError::Serialize(error.to_string()))?;
    let mut contents = bytes;
    contents.push(b'\n');
    write_new_file(path, &contents)
}

fn read_regular_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RecoveryError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    if !metadata.file_type().is_file() {
        return Err(RecoveryError::NotOwned(path.to_owned()));
    }
    let bytes = fs::read(path).map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| RecoveryError::Damaged(path.to_owned(), error.to_string()))
}

fn ensure_directory_chain(root: &Path, target: &Path) -> Result<(), RecoveryError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| RecoveryError::UnsafePath(target.to_owned()))?;
    let mut current = root.to_owned();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(RecoveryError::UnsafePath(target.to_owned()));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(RecoveryError::NotOwned(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|source| RecoveryError::Io(current.clone(), source))?;
            }
            Err(source) => return Err(RecoveryError::Io(current, source)),
        }
    }
    Ok(())
}

fn verify_existing_directory_chain(root: &Path, target: &Path) -> Result<(), RecoveryError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| RecoveryError::UnsafePath(target.to_owned()))?;
    require_directory(root)?;
    let mut current = root.to_owned();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(RecoveryError::UnsafePath(target.to_owned()));
        }
        current.push(component);
        require_directory(&current)?;
    }
    Ok(())
}

fn allocate_human_directory(parent: &Path, requested: &str) -> Result<PathBuf, RecoveryError> {
    require_directory(parent)?;
    let mut occupied = BTreeSet::new();
    for entry in
        fs::read_dir(parent).map_err(|source| RecoveryError::Io(parent.to_owned(), source))?
    {
        let entry = entry.map_err(|source| RecoveryError::Io(parent.to_owned(), source))?;
        occupied.insert(entry.file_name().to_string_lossy().to_lowercase());
    }
    for number in 1..=10_000 {
        let suffix = (number > 1).then(|| format!(" ({number})"));
        let component = bounded_human_component(requested, suffix.as_deref());
        if occupied.contains(&component.to_lowercase()) {
            continue;
        }
        let path = parent.join(&component);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                occupied.insert(component.to_lowercase());
            }
            Err(source) => return Err(RecoveryError::Io(path, source)),
        }
    }
    Err(RecoveryError::CannotAllocateHumanPath(parent.to_owned()))
}

fn bounded_human_component(value: &str, suffix: Option<&str>) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut spacing = false;
    for character in value.chars() {
        let invalid = character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            );
        if invalid || character.is_whitespace() {
            spacing = true;
        } else {
            if spacing && !normalized.is_empty() {
                normalized.push(' ');
            }
            spacing = false;
            normalized.push(character);
        }
    }
    let mut normalized = normalized.trim_matches([' ', '.']).to_owned();
    if normalized.is_empty() || normalized == "." || normalized == ".." {
        normalized = "release".into();
    }
    if is_windows_reserved(&normalized) {
        normalized.insert(0, '_');
    }
    let suffix = suffix.unwrap_or("");
    let suffix_chars = suffix.chars().count();
    let suffix_bytes = suffix.len();
    let suffix_units = suffix.encode_utf16().count();
    let mut bounded = String::new();
    for character in normalized.chars() {
        if bounded.chars().count() + 1 + suffix_chars > MAX_HUMAN_COMPONENT_CHARS
            || bounded.len() + character.len_utf8() + suffix_bytes > MAX_HUMAN_COMPONENT_UNITS
            || bounded.encode_utf16().count() + character.len_utf16() + suffix_units
                > MAX_HUMAN_COMPONENT_UNITS
        {
            break;
        }
        bounded.push(character);
    }
    let bounded = bounded.trim_end_matches([' ', '.']);
    format!("{bounded}{suffix}")
}

fn is_windows_reserved(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

pub fn retained_time_label(timestamp: u64) -> Result<String, RecoveryError> {
    let seconds = i64::try_from(timestamp).map_err(|_| {
        RecoveryError::InvalidMetadata("retention timestamp is outside the supported range".into())
    })?;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_date(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
    ))
}

fn retained_timestamp_component(timestamp: u64) -> Result<String, RecoveryError> {
    Ok(format!(
        "retained {}",
        retained_time_label(timestamp)?.replace(':', "-")
    ))
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn strict_tree_size(path: &Path) -> Result<u64, RecoveryError> {
    require_directory(path)?;
    let mut total = 0_u64;
    let entries =
        fs::read_dir(path).map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    for entry in entries {
        let entry = entry.map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)
            .map_err(|source| RecoveryError::Io(child.clone(), source))?;
        if metadata.file_type().is_dir() {
            total = total.saturating_add(strict_tree_size(&child)?);
        } else if metadata.file_type().is_file() {
            total = total.saturating_add(metadata.len());
        } else {
            return Err(RecoveryError::UnsafePath(child));
        }
    }
    Ok(total)
}

fn require_existing_directory(path: &Path) -> Result<(), RecoveryError> {
    require_directory(path).map_err(|error| match error {
        RecoveryError::Io(_, source) if source.kind() == std::io::ErrorKind::NotFound => {
            RecoveryError::InvalidLibraryRoot(path.to_owned())
        }
        other => other,
    })
}

fn require_directory(path: &Path) -> Result<(), RecoveryError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    if !metadata.file_type().is_dir() {
        return Err(RecoveryError::NotOwned(path.to_owned()));
    }
    Ok(())
}

fn require_exact_regular_file(path: &Path, expected: &[u8]) -> Result<(), RecoveryError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            RecoveryError::NotOwned(path.to_owned())
        } else {
            RecoveryError::Io(path.to_owned(), source)
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(RecoveryError::NotOwned(path.to_owned()));
    }
    let contents = fs::read(path).map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    if contents != expected {
        return Err(RecoveryError::NotOwned(path.to_owned()));
    }
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), RecoveryError> {
    let parent = path
        .parent()
        .ok_or_else(|| RecoveryError::UnsafePath(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| RecoveryError::Io(parent.to_owned(), source))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| RecoveryError::Io(parent.to_owned(), source))?;
    serde_json::to_writer_pretty(&mut temporary, value)
        .map_err(|error| RecoveryError::Serialize(error.to_string()))?;
    temporary
        .write_all(b"\n")
        .and_then(|()| temporary.as_file_mut().sync_all())
        .map_err(|source| RecoveryError::Io(path.to_owned(), source))?;
    temporary
        .persist(path)
        .map_err(|error| RecoveryError::Io(path.to_owned(), error.error))?;
    Ok(())
}

#[derive(Debug)]
pub enum RecoveryError {
    InvalidLibraryRoot(PathBuf),
    NotOwned(PathBuf),
    UnsafePath(PathBuf),
    IdentityMismatch(PathBuf),
    IdentityCollision(PathBuf),
    CannotAllocateHumanPath(PathBuf),
    InvalidMetadata(String),
    Damaged(PathBuf, String),
    Io(PathBuf, std::io::Error),
    Serialize(String),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryRoot(path) => write!(
                formatter,
                "library root is not an existing directory: {}",
                path.display()
            ),
            Self::NotOwned(path) => write!(
                formatter,
                "refusing to use unmarked or invalid recovery storage at {}",
                path.display()
            ),
            Self::UnsafePath(path) => {
                write!(formatter, "unsafe recovery path: {}", path.display())
            }
            Self::IdentityMismatch(path) => write!(
                formatter,
                "active receipt does not match recovery lineage at {}",
                path.display()
            ),
            Self::IdentityCollision(path) => write!(
                formatter,
                "recovery identity already exists at {}",
                path.display()
            ),
            Self::CannotAllocateHumanPath(path) => write!(
                formatter,
                "cannot allocate a unique recovery directory under {}",
                path.display()
            ),
            Self::InvalidMetadata(cause) => write!(formatter, "invalid recovery metadata: {cause}"),
            Self::Damaged(path, cause) => {
                write!(
                    formatter,
                    "damaged recovery metadata at {}: {cause}",
                    path.display()
                )
            }
            Self::Io(path, source) => write!(formatter, "{}: {source}", path.display()),
            Self::Serialize(cause) => write!(formatter, "cannot encode recovery metadata: {cause}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn creates_owned_navidrome_excluded_store() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();

        let store = RecoveryStore::create_or_open(&library).unwrap();

        assert_eq!(
            fs::read(store.root().join(MARKER)).unwrap(),
            MARKER_CONTENTS
        );
        assert_eq!(fs::read(store.root().join(NAVIDROME_IGNORE)).unwrap(), b"");
        assert_eq!(RecoveryStore::open_existing(&library).unwrap(), Some(store));
    }

    #[test]
    fn refuses_to_claim_even_an_empty_unmarked_directory() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        fs::create_dir(library.join(RECOVERY_DIRECTORY)).unwrap();

        let error = RecoveryStore::create_or_open(&library).unwrap_err();

        assert!(error.to_string().contains("unmarked or invalid"));
    }

    #[test]
    fn refuses_nonblank_navidrome_ignore() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        fs::write(store.root().join(NAVIDROME_IGNORE), b"not blank").unwrap();

        let error = RecoveryStore::open_existing(&library).unwrap_err();

        assert!(error.to_string().contains("unmarked or invalid"));
    }

    #[test]
    fn missing_index_never_hides_existing_payload_storage() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        fs::create_dir(store.root().join(PAYLOADS)).unwrap();

        let error = store.load_index().unwrap_err();

        assert!(error.to_string().contains("index is missing"));
    }

    #[test]
    fn index_and_active_receipt_round_trip_with_identity_check() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let active = library.join("Artist/Album");
        fs::create_dir_all(&active).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lineage_id = new_lineage_id();
        let active_version_id = new_version_id();
        let retained_version_id = new_version_id();
        let lineage = ReleaseLineage {
            lineage_id: lineage_id.clone(),
            active_version_id: active_version_id.clone(),
            expected_active_path: PathBuf::from("Artist/Album"),
            retained_versions: vec![RetainedVersion {
                version_id: retained_version_id,
                historical_path: PathBuf::from("Artist/Old Album"),
                display_label: "Artist — Album".into(),
                retained_at: 100,
                protected_until: 200,
                size_bytes: 123,
                storage_path: PathBuf::from("payloads/Artist Album/retained 1970-01-01"),
            }],
        };
        let index = RecoveryIndex {
            schema: INDEX_SCHEMA,
            lineages: vec![lineage.clone()],
        };

        let lock = store.lock().unwrap();
        store.save_index(&lock, &index).unwrap();
        store
            .write_active_receipt(
                &lock,
                &active,
                &ActiveReceipt::new(lineage_id, active_version_id),
            )
            .unwrap();

        assert_eq!(store.load_index().unwrap(), index);
        assert_eq!(store.verify_active_lineage(&lineage).unwrap(), active);
    }

    #[test]
    fn prepared_retained_payload_is_marked_measured_and_safely_discarded() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lock = store.lock().unwrap();
        let lineage_id = new_lineage_id();
        let version_id = new_version_id();
        let prepared = store
            .prepare_retained_payload(&lock, &lineage_id, &version_id, "Artist — Album", 100, None)
            .unwrap();
        fs::create_dir(&prepared.payload).unwrap();
        fs::write(prepared.payload.join("track.flac"), b"audio").unwrap();

        assert_eq!(
            store
                .retained_size(&lineage_id, &version_id, &prepared.storage_path)
                .unwrap(),
            5
        );

        fs::remove_dir_all(&prepared.payload).unwrap();
        store
            .discard_prepared_payload(&lock, &lineage_id, &version_id, &prepared.storage_path)
            .unwrap();
        assert!(!prepared.payload.parent().unwrap().exists());
    }

    #[test]
    fn retained_payload_paths_are_human_bounded_and_collision_safe() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lock = store.lock().unwrap();
        let first = store
            .prepare_retained_payload(
                &lock,
                &new_lineage_id(),
                &new_version_id(),
                "Artist: <Album>?",
                1_787_953_330,
                None,
            )
            .unwrap();
        let lineage_storage = first.storage_path.parent().unwrap().to_owned();
        let second = store
            .prepare_retained_payload(
                &lock,
                &new_lineage_id(),
                &new_version_id(),
                "ignored for an existing lineage",
                1_787_953_330,
                Some(&lineage_storage),
            )
            .unwrap();

        assert_eq!(
            first.storage_path,
            PathBuf::from("payloads/Artist Album/retained 2026-08-28 21-42-10 UTC")
        );
        assert_eq!(
            second.storage_path,
            PathBuf::from("payloads/Artist Album/retained 2026-08-28 21-42-10 UTC (2)")
        );
        assert_eq!(first.storage_path.components().count(), 3);
    }

    #[test]
    fn human_components_cover_windows_names_unicode_and_limits() {
        assert_eq!(bounded_human_component("CON.txt", None), "_CON.txt");
        assert_eq!(
            bounded_human_component("  <bad>|name...  ", None),
            "bad name"
        );
        assert_eq!(bounded_human_component("***", None), "release");
        let bounded = bounded_human_component(&"é".repeat(200), Some(" (9999)"));
        assert!(bounded.chars().count() <= MAX_HUMAN_COMPONENT_CHARS);
        assert!(bounded.len() <= MAX_HUMAN_COMPONENT_UNITS);
        assert!(bounded.encode_utf16().count() <= MAX_HUMAN_COMPONENT_UNITS);
        assert!(bounded.ends_with(" (9999)"));
    }

    #[test]
    fn retention_times_have_stable_utc_labels() {
        assert_eq!(retained_time_label(0).unwrap(), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            retained_time_label(1_787_953_330).unwrap(),
            "2026-08-28 21:42:10 UTC"
        );
    }

    #[test]
    fn retained_payload_measurement_rejects_symlinks_or_special_objects() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lock = store.lock().unwrap();
        let lineage_id = new_lineage_id();
        let version_id = new_version_id();
        let prepared = store
            .prepare_retained_payload(&lock, &lineage_id, &version_id, "Artist — Album", 100, None)
            .unwrap();
        fs::create_dir(&prepared.payload).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink("outside", prepared.payload.join("unsafe")).unwrap();
        #[cfg(windows)]
        fs::write(prepared.payload.join("ordinary"), b"safe fallback").unwrap();

        #[cfg(unix)]
        assert!(matches!(
            store.retained_size(&lineage_id, &version_id, &prepared.storage_path),
            Err(RecoveryError::UnsafePath(_))
        ));
        #[cfg(windows)]
        assert_eq!(
            store
                .retained_size(&lineage_id, &version_id, &prepared.storage_path)
                .unwrap(),
            b"safe fallback".len() as u64
        );
    }

    #[cfg(unix)]
    #[test]
    fn retained_payload_operations_reject_a_symlinked_storage_ancestor() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lock = store.lock().unwrap();
        let lineage_id = new_lineage_id();
        let version_id = new_version_id();
        let prepared = store
            .prepare_retained_payload(&lock, &lineage_id, &version_id, "Artist — Album", 100, None)
            .unwrap();
        let payloads = store.root().join(PAYLOADS);
        let external = temporary.path().join("external-payloads");
        fs::rename(&payloads, &external).unwrap();
        std::os::unix::fs::symlink(&external, &payloads).unwrap();

        assert!(matches!(
            store.verify_retained_payload(&lineage_id, &version_id, &prepared.storage_path),
            Err(RecoveryError::NotOwned(path)) if path == payloads
        ));
        assert!(matches!(
            store.discard_prepared_payload(
                &lock,
                &lineage_id,
                &version_id,
                &prepared.storage_path
            ),
            Err(RecoveryError::NotOwned(path)) if path == payloads
        ));
    }

    #[test]
    fn identity_mismatch_stops_before_filesystem_movement() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let active = library.join("Artist/Album");
        fs::create_dir_all(&active).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lineage = ReleaseLineage {
            lineage_id: new_lineage_id(),
            active_version_id: new_version_id(),
            expected_active_path: PathBuf::from("Artist/Album"),
            retained_versions: Vec::new(),
        };
        let lock = store.lock().unwrap();
        store
            .write_active_receipt(
                &lock,
                &active,
                &ActiveReceipt::new(new_lineage_id(), new_version_id()),
            )
            .unwrap();

        let error = store.verify_active_lineage(&lineage).unwrap_err();

        assert!(matches!(error, RecoveryError::IdentityMismatch(path) if path == active));
    }

    #[test]
    fn active_receipt_must_be_a_regular_file() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let active = library.join("Artist/Album");
        fs::create_dir_all(active.join(ACTIVE_RECEIPT)).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();

        let error = store.read_active_receipt(&active).unwrap_err();

        assert!(error.to_string().contains("not a regular file"));
    }

    #[test]
    fn invalid_paths_and_duplicate_identifiers_never_enter_the_index() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        fs::create_dir(&library).unwrap();
        let store = RecoveryStore::create_or_open(&library).unwrap();
        let lock = store.lock().unwrap();
        let shared_id = new_version_id();
        let mut invalid = RecoveryIndex {
            schema: INDEX_SCHEMA,
            lineages: vec![ReleaseLineage {
                lineage_id: new_lineage_id(),
                active_version_id: shared_id.clone(),
                expected_active_path: PathBuf::from("../outside"),
                retained_versions: vec![RetainedVersion {
                    version_id: shared_id,
                    historical_path: PathBuf::from("Artist/Album"),
                    display_label: "Artist — Album".into(),
                    retained_at: 100,
                    protected_until: 200,
                    size_bytes: 123,
                    storage_path: PathBuf::from("payloads/Artist Album/retained 1970-01-01"),
                }],
            }],
        };

        let error = store.save_index(&lock, &invalid).unwrap_err();

        assert!(error.to_string().contains("unsafe recovery path"));
        assert!(!store.root().join(INDEX).exists());

        invalid.lineages[0].expected_active_path =
            PathBuf::from(RECOVERY_DIRECTORY).join("lineage");
        let error = store.save_index(&lock, &invalid).unwrap_err();

        assert!(error.to_string().contains("unsafe recovery path"));
        assert!(!store.root().join(INDEX).exists());

        invalid.lineages[0].expected_active_path = PathBuf::from("Artist/Album");
        let error = store.save_index(&lock, &invalid).unwrap_err();

        assert!(error.to_string().contains("duplicate version identifier"));
        assert!(!store.root().join(INDEX).exists());

        invalid.lineages[0].retained_versions[0].version_id = new_version_id();
        invalid.lineages[0].lineage_id = invalid.lineages[0].lineage_id.to_uppercase();
        let error = store.save_index(&lock, &invalid).unwrap_err();

        assert!(error.to_string().contains("not canonical"));
        assert!(!store.root().join(INDEX).exists());
    }

    #[test]
    fn maintenance_evicts_oldest_eligible_until_within_cap() {
        let lineage_id = new_lineage_id();
        let index = RecoveryIndex {
            schema: INDEX_SCHEMA,
            lineages: vec![ReleaseLineage {
                lineage_id: lineage_id.clone(),
                active_version_id: new_version_id(),
                expected_active_path: PathBuf::from("Artist/Album"),
                retained_versions: vec![
                    retained("old", 10, 10, 60),
                    retained("new", 20, 20, 50),
                    retained("protected", 5, 200, 80),
                ],
            }],
        };

        let plan = plan_maintenance(&index, 100, 100);

        assert_eq!(plan.usage_before, 190);
        assert_eq!(plan.usage_after, 80);
        assert_eq!(
            plan.evictions
                .iter()
                .map(|eviction| eviction.display_label.as_str())
                .collect::<Vec<_>>(),
            ["old", "new"]
        );
        assert_eq!(plan.earliest_protected_until, None);
        assert!(
            plan.evictions
                .iter()
                .all(|eviction| eviction.lineage_id == lineage_id)
        );
    }

    #[test]
    fn protected_versions_can_leave_usage_over_cap() {
        let index = RecoveryIndex {
            schema: INDEX_SCHEMA,
            lineages: vec![ReleaseLineage {
                lineage_id: new_lineage_id(),
                active_version_id: new_version_id(),
                expected_active_path: PathBuf::from("Artist/Album"),
                retained_versions: vec![retained("protected", 10, 200, 120)],
            }],
        };

        let plan = plan_maintenance(&index, 100, 100);

        assert!(plan.evictions.is_empty());
        assert_eq!(plan.usage_after, 120);
        assert_eq!(plan.earliest_protected_until, Some(200));
    }

    fn retained(
        label: &str,
        retained_at: u64,
        protected_until: u64,
        size_bytes: u64,
    ) -> RetainedVersion {
        RetainedVersion {
            version_id: new_version_id(),
            historical_path: PathBuf::from("Artist/Album"),
            display_label: label.into(),
            retained_at,
            protected_until,
            size_bytes,
            storage_path: PathBuf::from(format!("payloads/Artist Album/{label}")),
        }
    }
}
