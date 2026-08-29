use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::apply::publication::{
    PublicationError, create_parents, exclusive_rename, remove_empty_parents, same_filesystem,
};
use crate::recovery::{
    ACTIVE_RECEIPT, ActiveReceipt, RECOVERY_DIRECTORY, RecoveryError, RecoveryIndex, RecoveryStore,
    ReleaseLineage, RetainedVersion,
};

/// Inputs for restoring one retained version. The caller computes and checks
/// the effective protection deadline before constructing this request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreRequest {
    pub library_root: PathBuf,
    pub lineage_id: String,
    pub version_id: String,
    pub retained_at: u64,
    pub protected_until: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreReport {
    pub active_path: PathBuf,
    pub displaced_retained_path: PathBuf,
    pub displaced_version_id: String,
    pub displaced_display_label: String,
    pub displaced_retained_at: u64,
    pub displaced_protected_until: u64,
    pub cleanup_warning: Option<String>,
}

pub fn restore(request: &RestoreRequest) -> Result<RestoreReport, RestoreError> {
    restore_with(request, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestorePoint {
    ActiveRetained,
    SelectedActivated,
    BeforeIndexCommit,
}

fn restore_with(
    request: &RestoreRequest,
    mut checkpoint: impl FnMut(RestorePoint) -> Result<(), String>,
) -> Result<RestoreReport, RestoreError> {
    validate_request(request)?;
    let root = canonical_root(&request.library_root)?;
    let store = RecoveryStore::open_existing(&root)?
        .ok_or_else(|| RestoreError::NoRecoveryStore(root.clone()))?;
    let lock = store.lock()?;
    let original_index = store.load_index()?;
    let (lineage_index, lineage) = find_lineage(&original_index, &request.lineage_id)?;
    let (selected_index, selected) = find_selected(lineage, &request.version_id)?;
    if selected.version_id == lineage.active_version_id {
        return Err(RestoreError::IdentityMismatch(
            root.join(&lineage.expected_active_path),
        ));
    }

    let active_path = store.verify_active_lineage(lineage)?;
    validate_literal_directory(&root, &active_path)?;
    if active_path != root.join(&lineage.expected_active_path) {
        return Err(RestoreError::IdentityMismatch(active_path));
    }
    require_same_filesystem(&active_path, store.root())?;

    let selected_payload = store.verify_retained_payload(
        &lineage.lineage_id,
        &selected.version_id,
        &selected.storage_path,
    )?;
    store.retained_size(
        &lineage.lineage_id,
        &selected.version_id,
        &selected.storage_path,
    )?;
    let selected_parent = selected_payload
        .parent()
        .expect("verified retained payload always has a container");
    require_same_filesystem(&selected_payload, store.root())?;
    // A retained payload from the first replacement may have no receipt. Keep
    // the optional bytes before creating any transaction state so a malformed
    // receipt cannot strand a prepared container or newly-created parents.
    let selected_receipt_backup = read_optional_receipt(&selected_payload)?;

    let historical_path = root.join(&selected.historical_path);
    validate_relative_library_path(&root, &historical_path)?;
    if historical_path != active_path
        && (historical_path.starts_with(&active_path) || active_path.starts_with(&historical_path))
    {
        return Err(RestoreError::DestinationCollision(historical_path));
    }
    check_historical_collision(&historical_path, &active_path)?;
    refuse_other_active_path(&original_index, lineage_index, &selected.historical_path)?;
    let historical_parent = historical_path
        .parent()
        .ok_or_else(|| RestoreError::UnsafePath(historical_path.clone()))?;
    ensure_safe_parent_chain(&root, historical_parent)?;
    let created_parents = if historical_path == active_path {
        Vec::new()
    } else {
        let created = create_parents(&root, &historical_path)?;
        if let Err(error) = ensure_safe_parent_chain(&root, historical_parent) {
            remove_empty_parents(created);
            return Err(error);
        }
        created
    };
    require_same_filesystem(&selected_payload, historical_parent)?;

    let displaced_version_id = lineage.active_version_id.clone();
    let displaced_display_label = display_label(&lineage.expected_active_path);
    let lineage_storage = selected_parent
        .parent()
        .and_then(|path| path.strip_prefix(store.root()).ok())
        .ok_or_else(|| RestoreError::UnsafePath(selected_parent.to_owned()))?;
    let prepared = match store.prepare_retained_payload(
        &lock,
        &lineage.lineage_id,
        &displaced_version_id,
        &displaced_display_label,
        request.retained_at,
        Some(lineage_storage),
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            remove_empty_parents(created_parents);
            return Err(error.into());
        }
    };
    let prepared_payload = prepared.payload.clone();
    let mut selected_activated = false;

    if let Err(error) = exclusive_rename(&active_path, &prepared_payload) {
        return Err(cleanup_before_moves(
            error.into(),
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }
    let active_retained = true;
    if let Err(cause) = checkpoint(RestorePoint::ActiveRetained) {
        return Err(rollback(
            RestoreError::Checkpoint(cause),
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }

    let displaced_size = match store.retained_size(
        &lineage.lineage_id,
        &displaced_version_id,
        &prepared.storage_path,
    ) {
        Ok(size) => size,
        Err(error) => {
            return Err(rollback(
                error.into(),
                &root,
                &active_path,
                &historical_path,
                &selected_payload,
                &prepared_payload,
                &selected_receipt_backup,
                active_retained,
                selected_activated,
                created_parents,
                &store,
                &lock,
                &lineage.lineage_id,
                &displaced_version_id,
                &prepared.storage_path,
            ));
        }
    };

    if let Err(error) = exclusive_rename(&selected_payload, &historical_path) {
        return Err(rollback(
            error.into(),
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }
    selected_activated = true;
    if let Err(error) = store.write_active_receipt(
        &lock,
        &historical_path,
        &ActiveReceipt::new(&lineage.lineage_id, &selected.version_id),
    ) {
        return Err(rollback(
            error.into(),
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }
    if let Err(cause) = checkpoint(RestorePoint::SelectedActivated) {
        return Err(rollback(
            RestoreError::Checkpoint(cause),
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }

    let mut updated_index = original_index.clone();
    update_index(
        &mut updated_index,
        lineage_index,
        selected_index,
        selected,
        lineage,
        &displaced_version_id,
        &displaced_display_label,
        request,
        displaced_size,
        prepared.storage_path.clone(),
    )?;
    if let Err(cause) = checkpoint(RestorePoint::BeforeIndexCommit) {
        return Err(rollback(
            RestoreError::Checkpoint(cause),
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }
    if let Err(error) = store.save_index(&lock, &updated_index) {
        return Err(rollback_after_index_failure(
            error.into(),
            &original_index,
            &root,
            &active_path,
            &historical_path,
            &selected_payload,
            &prepared_payload,
            &selected_receipt_backup,
            active_retained,
            selected_activated,
            created_parents,
            &store,
            &lock,
            &lineage.lineage_id,
            &displaced_version_id,
            &prepared.storage_path,
        ));
    }

    let cleanup_warning = store
        .discard_prepared_payload(
            &lock,
            &lineage.lineage_id,
            &selected.version_id,
            &selected.storage_path,
        )
        .err()
        .map(|error| format!("could not remove restored version's empty recovery marker: {error}"));
    Ok(RestoreReport {
        active_path: historical_path,
        displaced_retained_path: prepared_payload,
        displaced_version_id,
        displaced_display_label,
        displaced_retained_at: request.retained_at,
        displaced_protected_until: request.protected_until,
        cleanup_warning,
    })
}

fn validate_request(request: &RestoreRequest) -> Result<(), RestoreError> {
    if request.protected_until < request.retained_at {
        return Err(RestoreError::InvalidRequest(
            "protection deadline is before the retention time".into(),
        ));
    }
    let root = request
        .library_root
        .canonicalize()
        .map_err(|source| RecoveryError::Io(request.library_root.clone(), source))?;
    let metadata =
        fs::symlink_metadata(&root).map_err(|source| RecoveryError::Io(root.clone(), source))?;
    if !metadata.file_type().is_dir() {
        return Err(RestoreError::UnsafePath(root));
    }
    Ok(())
}

fn canonical_root(path: &Path) -> Result<PathBuf, RestoreError> {
    path.canonicalize()
        .map_err(|source| RecoveryError::Io(path.to_owned(), source).into())
}

fn require_same_filesystem(left: &Path, right: &Path) -> Result<(), RestoreError> {
    if same_filesystem(left, right)? {
        Ok(())
    } else {
        Err(RestoreError::DifferentFilesystem {
            left: left.to_owned(),
            right: right.to_owned(),
        })
    }
}

fn find_lineage<'a>(
    index: &'a RecoveryIndex,
    lineage_id: &str,
) -> Result<(usize, &'a ReleaseLineage), RestoreError> {
    index
        .lineages
        .iter()
        .enumerate()
        .find(|(_, lineage)| lineage.lineage_id == lineage_id)
        .ok_or_else(|| RestoreError::IdentityMismatch(PathBuf::from(lineage_id)))
}

fn find_selected<'a>(
    lineage: &'a ReleaseLineage,
    version_id: &str,
) -> Result<(usize, &'a RetainedVersion), RestoreError> {
    lineage
        .retained_versions
        .iter()
        .enumerate()
        .find(|(_, version)| version.version_id == version_id)
        .ok_or_else(|| RestoreError::IdentityMismatch(PathBuf::from(version_id)))
}

fn check_historical_collision(path: &Path, active: &Path) -> Result<(), RestoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) if path == active => Ok(()),
        Ok(_) => Err(RestoreError::DestinationCollision(path.to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RecoveryError::Io(path.to_owned(), source).into()),
    }
}

fn refuse_other_active_path(
    index: &RecoveryIndex,
    current: usize,
    historical: &Path,
) -> Result<(), RestoreError> {
    if index
        .lineages
        .iter()
        .enumerate()
        .any(|(position, lineage)| {
            position != current && lineage.expected_active_path == historical
        })
    {
        return Err(RestoreError::IdentityMismatch(historical.to_owned()));
    }
    Ok(())
}

fn validate_relative_library_path(root: &Path, path: &Path) -> Result<(), RestoreError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| RestoreError::UnsafePath(path.to_owned()))?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            !matches!(component, Component::Normal(value) if value != RECOVERY_DIRECTORY)
        })
    {
        return Err(RestoreError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn ensure_safe_parent_chain(root: &Path, parent: &Path) -> Result<(), RestoreError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| RestoreError::UnsafePath(parent.to_owned()))?;
    let mut current = root.to_owned();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(RestoreError::UnsafePath(parent.to_owned()));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(RestoreError::UnsafePath(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => return Err(RecoveryError::Io(current, source).into()),
        }
    }
    Ok(())
}

fn validate_literal_directory(root: &Path, directory: &Path) -> Result<(), RestoreError> {
    validate_relative_library_path(root, directory)?;
    let relative = directory
        .strip_prefix(root)
        .expect("validated path is below root");
    let mut current = root.to_owned();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(RestoreError::UnsafePath(directory.to_owned()));
        };
        current.push(value);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|source| RecoveryError::Io(current.clone(), source))?;
        if !metadata.file_type().is_dir() {
            return Err(RestoreError::UnsafePath(current));
        }
    }
    if directory
        .canonicalize()
        .map_err(|source| RestoreError::from(RecoveryError::Io(directory.to_owned(), source)))?
        != *directory
    {
        return Err(RestoreError::UnsafePath(directory.to_owned()));
    }
    Ok(())
}

fn display_label(path: &Path) -> String {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match components.as_slice() {
        [] => "Retained release".into(),
        [release] => release.to_string(),
        components => format!(
            "{} — {}",
            components[components.len() - 2],
            components[components.len() - 1]
        ),
    }
}

fn read_optional_receipt(directory: &Path) -> Result<Option<Vec<u8>>, RestoreError> {
    let path = directory.join(ACTIVE_RECEIPT);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(fs::read(&path)
            .map(Some)
            .map_err(|source| RecoveryError::Io(path, source))?),
        Ok(_) => Err(RestoreError::UnsafePath(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(RecoveryError::Io(path, source).into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_index(
    index: &mut RecoveryIndex,
    lineage_index: usize,
    selected_index: usize,
    selected: &RetainedVersion,
    lineage: &ReleaseLineage,
    displaced_version_id: &str,
    displaced_display_label: &str,
    request: &RestoreRequest,
    displaced_size: u64,
    displaced_storage_path: PathBuf,
) -> Result<(), RestoreError> {
    let lineage_entry = index
        .lineages
        .get_mut(lineage_index)
        .ok_or_else(|| RestoreError::IdentityMismatch(PathBuf::from(&request.lineage_id)))?;
    let old_active_path = lineage.expected_active_path.clone();
    let selected_path = selected.historical_path.clone();
    lineage_entry.active_version_id = selected.version_id.clone();
    lineage_entry.expected_active_path = selected_path;
    lineage_entry.retained_versions.remove(selected_index);
    lineage_entry.retained_versions.push(RetainedVersion {
        version_id: displaced_version_id.to_owned(),
        historical_path: old_active_path,
        display_label: displaced_display_label.to_owned(),
        retained_at: request.retained_at,
        protected_until: request.protected_until,
        size_bytes: displaced_size,
        storage_path: displaced_storage_path,
    });
    Ok(())
}

fn cleanup_before_moves(
    original: RestoreError,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    lineage_id: &str,
    version_id: &str,
    storage_path: &Path,
) -> RestoreError {
    remove_empty_parents(created_parents);
    match store.discard_prepared_payload(lock, lineage_id, version_id, storage_path) {
        Ok(()) => original,
        Err(error) => RestoreError::Rollback {
            original: Box::new(original),
            failures: vec![format!(
                "cannot remove prepared recovery container: {error}"
            )],
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn rollback(
    original: RestoreError,
    _root: &Path,
    active_path: &Path,
    historical_path: &Path,
    selected_payload: &Path,
    prepared_payload: &Path,
    receipt_backup: &Option<Vec<u8>>,
    active_retained: bool,
    selected_activated: bool,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    lineage_id: &str,
    displaced_version_id: &str,
    prepared_storage_path: &Path,
) -> RestoreError {
    rollback_inner(
        original,
        active_path,
        historical_path,
        selected_payload,
        prepared_payload,
        receipt_backup,
        active_retained,
        selected_activated,
        created_parents,
        store,
        lock,
        lineage_id,
        displaced_version_id,
        prepared_storage_path,
    )
}

#[allow(clippy::too_many_arguments)]
fn rollback_after_index_failure(
    original: RestoreError,
    original_index: &RecoveryIndex,
    _root: &Path,
    active_path: &Path,
    historical_path: &Path,
    selected_payload: &Path,
    prepared_payload: &Path,
    receipt_backup: &Option<Vec<u8>>,
    active_retained: bool,
    selected_activated: bool,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    lineage_id: &str,
    displaced_version_id: &str,
    prepared_storage_path: &Path,
) -> RestoreError {
    let rollback_result = rollback_inner(
        original,
        active_path,
        historical_path,
        selected_payload,
        prepared_payload,
        receipt_backup,
        active_retained,
        selected_activated,
        created_parents,
        store,
        lock,
        lineage_id,
        displaced_version_id,
        prepared_storage_path,
    );
    if rollback_result.rollback_incomplete() {
        return rollback_result;
    }
    match store.save_index(lock, original_index) {
        Ok(()) => rollback_result,
        Err(error) => RestoreError::Rollback {
            original: Box::new(rollback_result),
            failures: vec![format!(
                "cannot restore the previous recovery index: {error}"
            )],
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn rollback_inner(
    original: RestoreError,
    active_path: &Path,
    historical_path: &Path,
    selected_payload: &Path,
    prepared_payload: &Path,
    receipt_backup: &Option<Vec<u8>>,
    active_retained: bool,
    selected_activated: bool,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    lineage_id: &str,
    displaced_version_id: &str,
    prepared_storage_path: &Path,
) -> RestoreError {
    let mut failures = Vec::new();
    if selected_activated {
        if let Err(error) = exclusive_rename(historical_path, selected_payload) {
            failures.push(format!(
                "cannot return selected version to recovery: {error}"
            ));
        } else if let Err(error) = restore_receipt(selected_payload, receipt_backup) {
            failures.push(format!("cannot restore selected version receipt: {error}"));
        }
    }
    if active_retained && let Err(error) = exclusive_rename(prepared_payload, active_path) {
        failures.push(format!("cannot restore displaced active release: {error}"));
    }
    if !prepared_payload.exists()
        && let Err(error) = store.discard_prepared_payload(
            lock,
            lineage_id,
            displaced_version_id,
            prepared_storage_path,
        )
    {
        failures.push(format!(
            "cannot remove prepared recovery container: {error}"
        ));
    }
    remove_empty_parents(created_parents);
    if failures.is_empty() {
        original
    } else {
        RestoreError::Rollback {
            original: Box::new(original),
            failures,
        }
    }
}

fn restore_receipt(directory: &Path, backup: &Option<Vec<u8>>) -> std::io::Result<()> {
    let path = directory.join(ACTIVE_RECEIPT);
    match backup {
        Some(contents) => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)?;
            file.write_all(contents)?;
            file.sync_all()
        }
        None => match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(path),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "restored receipt is not a regular file",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
    }
}

#[derive(Debug)]
pub enum RestoreError {
    InvalidRequest(String),
    NoRecoveryStore(PathBuf),
    UnsafePath(PathBuf),
    DestinationCollision(PathBuf),
    DifferentFilesystem {
        left: PathBuf,
        right: PathBuf,
    },
    IdentityMismatch(PathBuf),
    Recovery(RecoveryError),
    Publication(PublicationError),
    Checkpoint(String),
    Rollback {
        original: Box<RestoreError>,
        failures: Vec<String>,
    },
}

impl RestoreError {
    pub fn rollback_incomplete(&self) -> bool {
        matches!(self, Self::Rollback { .. })
    }
}

impl From<RecoveryError> for RestoreError {
    fn from(error: RecoveryError) -> Self {
        Self::Recovery(error)
    }
}

impl From<PublicationError> for RestoreError {
    fn from(error: PublicationError) -> Self {
        Self::Publication(error)
    }
}

impl fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(cause) => write!(formatter, "invalid restore request: {cause}"),
            Self::NoRecoveryStore(path) => {
                write!(
                    formatter,
                    "no recovery store exists under {}",
                    path.display()
                )
            }
            Self::UnsafePath(path) => write!(formatter, "unsafe restore path: {}", path.display()),
            Self::DestinationCollision(path) => write!(
                formatter,
                "restore historical path is occupied by unrelated content: {}",
                path.display()
            ),
            Self::DifferentFilesystem { left, right } => write!(
                formatter,
                "restore requires one filesystem, but {} and {} are on different filesystems",
                left.display(),
                right.display()
            ),
            Self::IdentityMismatch(path) => write!(
                formatter,
                "restore identity does not match recovery history at {}",
                path.display()
            ),
            Self::Recovery(error) => write!(formatter, "{error}"),
            Self::Publication(error) => write!(formatter, "{error}"),
            Self::Checkpoint(cause) => write!(formatter, "restore interrupted: {cause}"),
            Self::Rollback { original, failures } => write!(
                formatter,
                "{original}; restore rollback was incomplete: {}",
                failures.join("; ")
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;
    use crate::recovery::{ReleaseLineage, RetainedVersion, new_lineage_id, new_version_id};

    #[test]
    fn restores_same_path_and_rewrites_missing_receipt() {
        let fixture = Fixture::new(false);

        let report = restore(&fixture.request).unwrap();

        assert_eq!(report.active_path, fixture.active);
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"old");
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let index = store.load_index().unwrap();
        let lineage = &index.lineages[0];
        assert_eq!(lineage.active_version_id, fixture.selected_id);
        assert_eq!(lineage.expected_active_path, Path::new("Artist/Album"));
        assert_eq!(lineage.retained_versions.len(), 1);
        assert_eq!(report.displaced_version_id, fixture.current_id);
        assert_eq!(lineage.retained_versions[0].version_id, fixture.current_id);
        assert_eq!(
            lineage.retained_versions[0].historical_path,
            Path::new("Artist/Album")
        );
        assert_eq!(lineage.retained_versions[0].retained_at, 100);
        assert_eq!(lineage.retained_versions[0].protected_until, 200);
        assert_eq!(
            store.read_active_receipt(&fixture.active).unwrap(),
            ActiveReceipt::new(fixture.lineage_id, fixture.selected_id)
        );
    }

    #[test]
    fn restores_to_historical_path_and_keeps_displaced_active() {
        let fixture = Fixture::new(true);
        let historical = fixture.library.join("Artist/Old Album");

        let report = restore(&fixture.request).unwrap();

        assert_eq!(report.active_path, historical);
        assert!(!fixture.active.exists());
        assert_eq!(fs::read(historical.join("track")).unwrap(), b"old");
        assert_eq!(report.displaced_display_label, "Artist — Album");
        assert_eq!(report.displaced_retained_at, 100);
        assert_eq!(report.displaced_protected_until, 200);
        assert_eq!(
            fs::read(report.displaced_retained_path.join("track")).unwrap(),
            b"current"
        );
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let lineage = &store.load_index().unwrap().lineages[0];
        assert_eq!(lineage.expected_active_path, Path::new("Artist/Old Album"));
        assert_eq!(
            lineage.retained_versions[0].historical_path,
            Path::new("Artist/Album")
        );
    }

    #[test]
    fn occupied_historical_path_is_refused_without_movement() {
        let fixture = Fixture::new(true);
        let historical = fixture.library.join("Artist/Old Album");
        fs::create_dir_all(&historical).unwrap();
        fs::write(historical.join("unrelated"), b"keep").unwrap();

        let error = restore(&fixture.request).unwrap_err();

        assert!(matches!(error, RestoreError::DestinationCollision(path) if path == historical));
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"current");
        assert_eq!(fs::read(historical.join("unrelated")).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_selected_tree_is_refused_before_movement() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new(false);
        symlink("track", fixture.retained_payload().join("link")).unwrap();

        let error = restore(&fixture.request).unwrap_err();

        assert!(matches!(
            error,
            RestoreError::Recovery(RecoveryError::UnsafePath(_))
        ));
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"current");
        assert_eq!(
            fs::read(fixture.retained_payload().join("track")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn mismatched_active_receipt_is_refused_without_movement() {
        let fixture = Fixture::new(false);
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let lock = store.lock().unwrap();
        store
            .write_active_receipt(
                &lock,
                &fixture.active,
                &ActiveReceipt::new(new_lineage_id(), new_version_id()),
            )
            .unwrap();
        drop(lock);

        let error = restore(&fixture.request).unwrap_err();

        assert!(
            matches!(error, RestoreError::Recovery(RecoveryError::IdentityMismatch(path)) if path == fixture.active)
        );
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"current");
    }

    #[test]
    fn failure_after_selected_activation_rolls_back_moves_receipt_and_index() {
        let fixture = Fixture::new(true);
        let historical = fixture.library.join("Artist/Old Album");
        let error = restore_with(&fixture.request, |point| {
            (point != RestorePoint::SelectedActivated)
                .then_some(())
                .ok_or_else(|| "injected after selected activation".into())
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected after selected activation")
        );
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"current");
        assert!(!historical.exists());
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let index = store.load_index().unwrap();
        assert_eq!(index.lineages[0].active_version_id, fixture.current_id);
        assert_eq!(
            store.read_active_receipt(&fixture.active).unwrap(),
            ActiveReceipt::new(fixture.lineage_id, fixture.current_id)
        );
    }

    #[test]
    fn failure_before_index_commit_rolls_back_same_path_exactly() {
        let fixture = Fixture::new(false);
        let error = restore_with(&fixture.request, |point| {
            (point != RestorePoint::BeforeIndexCommit)
                .then_some(())
                .ok_or_else(|| "injected before index commit".into())
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected before index commit"));
        assert_eq!(fs::read(fixture.active.join("track")).unwrap(), b"current");
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let index = store.load_index().unwrap();
        assert_eq!(index.lineages[0].active_version_id, fixture.current_id);
        assert!(
            index.lineages[0].retained_versions[0]
                .storage_path
                .starts_with("payloads")
        );
    }

    struct Fixture {
        _temporary: TempDir,
        library: PathBuf,
        active: PathBuf,
        lineage_id: String,
        current_id: String,
        selected_id: String,
        request: RestoreRequest,
    }

    impl Fixture {
        fn new(relocate: bool) -> Self {
            let temporary = TempDir::new().unwrap();
            let unresolved_library = temporary.path().join("library");
            fs::create_dir(&unresolved_library).unwrap();
            let library = unresolved_library.canonicalize().unwrap();
            let active = library.join("Artist/Album");
            let historical = if relocate {
                PathBuf::from("Artist/Old Album")
            } else {
                PathBuf::from("Artist/Album")
            };
            fs::create_dir_all(&active).unwrap();
            fs::write(active.join("track"), b"current").unwrap();
            let store = RecoveryStore::create_or_open(&library).unwrap();
            let lock = store.lock().unwrap();
            let lineage_id = new_lineage_id();
            let current_id = new_version_id();
            let selected_id = new_version_id();
            let prepared = store
                .prepare_retained_payload(
                    &lock,
                    &lineage_id,
                    &selected_id,
                    "Artist — Old Album",
                    10,
                    None,
                )
                .unwrap();
            fs::create_dir(&prepared.payload).unwrap();
            fs::write(prepared.payload.join("track"), b"old").unwrap();
            let mut index = RecoveryIndex::default();
            index.lineages.push(ReleaseLineage {
                lineage_id: lineage_id.clone(),
                active_version_id: current_id.clone(),
                expected_active_path: PathBuf::from("Artist/Album"),
                retained_versions: vec![RetainedVersion {
                    version_id: selected_id.clone(),
                    historical_path: historical,
                    display_label: "Artist — Old Album".into(),
                    retained_at: 10,
                    protected_until: 20,
                    size_bytes: 3,
                    storage_path: prepared.storage_path,
                }],
            });
            store.save_index(&lock, &index).unwrap();
            store
                .write_active_receipt(
                    &lock,
                    &active,
                    &ActiveReceipt::new(&lineage_id, &current_id),
                )
                .unwrap();
            drop(lock);
            Self {
                _temporary: temporary,
                library: library.clone(),
                active,
                lineage_id: lineage_id.clone(),
                current_id,
                selected_id: selected_id.clone(),
                request: RestoreRequest {
                    library_root: library,
                    lineage_id,
                    version_id: selected_id,
                    retained_at: 100,
                    protected_until: 200,
                },
            }
        }

        fn retained_payload(&self) -> PathBuf {
            let store = RecoveryStore::open_existing(&self.library)
                .unwrap()
                .unwrap();
            let index = store.load_index().unwrap();
            store
                .retained_payload_path(&index.lineages[0].retained_versions[0].storage_path)
                .unwrap()
        }
    }
}
