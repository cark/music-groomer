use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::apply::publication::{
    PublicationError, create_parents, exclusive_rename, remove_empty_parents,
};
use crate::recovery::{
    ACTIVE_RECEIPT, ActiveReceipt, RecoveryError, RecoveryIndex, RecoveryStore, ReleaseLineage,
    RetainedVersion, new_lineage_id, new_version_id,
};

use super::ReplacementContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementSwap {
    pub context: ReplacementContext,
    pub prepared_replacement: PathBuf,
    pub display_label: String,
    pub retained_at: u64,
    pub protected_until: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementSwapReport {
    pub destination: PathBuf,
    pub retained_path: PathBuf,
    pub lineage_id: String,
    pub active_version_id: String,
    pub retained_version_id: String,
    pub retained_size_bytes: u64,
    pub display_label: String,
    pub retained_at: u64,
    pub protected_until: u64,
}

pub fn swap_prepared(swap: &ReplacementSwap) -> Result<ReplacementSwapReport, SwapError> {
    swap_prepared_with(swap, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwapPoint {
    ActiveRetained,
    ReplacementActivated,
    BeforeIndexCommit,
}

fn swap_prepared_with(
    swap: &ReplacementSwap,
    mut checkpoint: impl FnMut(SwapPoint) -> Result<(), String>,
) -> Result<ReplacementSwapReport, SwapError> {
    validate_request(swap)?;
    let store = RecoveryStore::create_or_open(&swap.context.library_root)?;
    let lock = store.lock()?;
    let mut index = store.load_index()?;
    let identity = resolve_identity(&store, &index, &swap.context)?;
    refuse_index_destination_collision(&index, identity.lineage_index, &swap.context)?;

    let active_version_id = new_version_id();
    let prepared_retained = store.prepare_retained_payload(
        &lock,
        &identity.lineage_id,
        &identity.retained_version_id,
        &swap.display_label,
        swap.retained_at,
        identity.lineage_storage.as_deref(),
    )?;
    let retained_path = prepared_retained.payload.clone();
    let created_parents =
        match create_parents(&swap.context.library_root, &swap.context.destination) {
            Ok(paths) => paths,
            Err(error) => {
                return Err(cleanup_before_first_move(
                    error.into(),
                    Vec::new(),
                    &store,
                    &lock,
                    &identity.lineage_id,
                    &identity.retained_version_id,
                    &prepared_retained.storage_path,
                ));
            }
        };

    if let Err(error) = store.write_active_receipt(
        &lock,
        &swap.prepared_replacement,
        &ActiveReceipt::new(&identity.lineage_id, &active_version_id),
    ) {
        return Err(cleanup_before_first_move(
            error.into(),
            created_parents,
            &store,
            &lock,
            &identity.lineage_id,
            &identity.retained_version_id,
            &prepared_retained.storage_path,
        ));
    }

    if let Err(error) = exclusive_rename(&swap.context.active_path, &retained_path) {
        return Err(cleanup_before_first_move(
            error.into(),
            created_parents,
            &store,
            &lock,
            &identity.lineage_id,
            &identity.retained_version_id,
            &prepared_retained.storage_path,
        ));
    }

    if let Err(cause) = checkpoint(SwapPoint::ActiveRetained) {
        return Err(rollback(
            SwapError::Checkpoint(cause),
            None,
            swap,
            &retained_path,
            created_parents,
            &store,
            &lock,
            &identity,
            &prepared_retained.storage_path,
        ));
    }

    let retained_size_bytes = match store.retained_size(
        &identity.lineage_id,
        &identity.retained_version_id,
        &prepared_retained.storage_path,
    ) {
        Ok(size) => size,
        Err(error) => {
            return Err(rollback(
                error.into(),
                None,
                swap,
                &retained_path,
                created_parents,
                &store,
                &lock,
                &identity,
                &prepared_retained.storage_path,
            ));
        }
    };

    if let Err(error) = exclusive_rename(&swap.prepared_replacement, &swap.context.destination) {
        return Err(rollback(
            error.into(),
            None,
            swap,
            &retained_path,
            created_parents,
            &store,
            &lock,
            &identity,
            &prepared_retained.storage_path,
        ));
    }

    if let Err(cause) = checkpoint(SwapPoint::ReplacementActivated) {
        return Err(rollback(
            SwapError::Checkpoint(cause),
            Some(&swap.context.destination),
            swap,
            &retained_path,
            created_parents,
            &store,
            &lock,
            &identity,
            &prepared_retained.storage_path,
        ));
    }

    update_index(
        &mut index,
        &identity,
        swap,
        &active_version_id,
        retained_size_bytes,
        prepared_retained.storage_path.clone(),
    );
    if let Err(cause) = checkpoint(SwapPoint::BeforeIndexCommit) {
        return Err(rollback(
            SwapError::Checkpoint(cause),
            Some(&swap.context.destination),
            swap,
            &retained_path,
            created_parents,
            &store,
            &lock,
            &identity,
            &prepared_retained.storage_path,
        ));
    }
    if let Err(error) = store.save_index(&lock, &index) {
        return Err(rollback(
            error.into(),
            Some(&swap.context.destination),
            swap,
            &retained_path,
            created_parents,
            &store,
            &lock,
            &identity,
            &prepared_retained.storage_path,
        ));
    }

    Ok(ReplacementSwapReport {
        destination: swap.context.destination.clone(),
        retained_path,
        lineage_id: identity.lineage_id,
        active_version_id,
        retained_version_id: identity.retained_version_id,
        retained_size_bytes,
        display_label: swap.display_label.clone(),
        retained_at: swap.retained_at,
        protected_until: swap.protected_until,
    })
}

struct Identity {
    lineage_index: Option<usize>,
    lineage_id: String,
    retained_version_id: String,
    lineage_storage: Option<PathBuf>,
}

fn resolve_identity(
    store: &RecoveryStore,
    index: &RecoveryIndex,
    context: &ReplacementContext,
) -> Result<Identity, SwapError> {
    let receipt_path = context.active_path.join(ACTIVE_RECEIPT);
    match fs::symlink_metadata(&receipt_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if index
                .lineages
                .iter()
                .any(|lineage| lineage.expected_active_path == context.historical_path)
            {
                return Err(SwapError::IdentityMismatch(context.active_path.clone()));
            }
            Ok(Identity {
                lineage_index: None,
                lineage_id: new_lineage_id(),
                retained_version_id: new_version_id(),
                lineage_storage: None,
            })
        }
        Err(source) => Err(RecoveryError::Io(receipt_path, source).into()),
        Ok(_) => {
            let receipt = store.read_active_receipt(&context.active_path)?;
            let (lineage_index, lineage) = index
                .lineages
                .iter()
                .enumerate()
                .find(|(_, lineage)| lineage.lineage_id == receipt.lineage_id)
                .ok_or_else(|| SwapError::IdentityMismatch(context.active_path.clone()))?;
            if lineage.expected_active_path != context.historical_path
                || lineage.active_version_id != receipt.version_id
            {
                return Err(SwapError::IdentityMismatch(context.active_path.clone()));
            }
            store.verify_active_lineage(lineage)?;
            Ok(Identity {
                lineage_index: Some(lineage_index),
                lineage_id: receipt.lineage_id,
                retained_version_id: receipt.version_id,
                lineage_storage: lineage
                    .retained_versions
                    .first()
                    .and_then(|version| version.storage_path.parent())
                    .map(Path::to_owned),
            })
        }
    }
}

fn refuse_index_destination_collision(
    index: &RecoveryIndex,
    current: Option<usize>,
    context: &ReplacementContext,
) -> Result<(), SwapError> {
    let relative = context
        .destination
        .strip_prefix(&context.library_root)
        .map_err(|_| SwapError::UnsafePath(context.destination.clone()))?;
    if index
        .lineages
        .iter()
        .enumerate()
        .any(|(position, lineage)| {
            Some(position) != current && lineage.expected_active_path == relative
        })
    {
        return Err(SwapError::IdentityMismatch(context.destination.clone()));
    }
    Ok(())
}

fn update_index(
    index: &mut RecoveryIndex,
    identity: &Identity,
    swap: &ReplacementSwap,
    active_version_id: &str,
    retained_size_bytes: u64,
    storage_path: PathBuf,
) {
    let retained = RetainedVersion {
        version_id: identity.retained_version_id.clone(),
        historical_path: swap.context.historical_path.clone(),
        display_label: swap.display_label.clone(),
        retained_at: swap.retained_at,
        protected_until: swap.protected_until,
        size_bytes: retained_size_bytes,
        storage_path,
    };
    let expected_active_path = swap
        .context
        .destination
        .strip_prefix(&swap.context.library_root)
        .expect("validated destination is inside the library")
        .to_owned();
    if let Some(position) = identity.lineage_index {
        let lineage = &mut index.lineages[position];
        lineage.active_version_id = active_version_id.to_owned();
        lineage.expected_active_path = expected_active_path;
        lineage.retained_versions.push(retained);
    } else {
        index.lineages.push(ReleaseLineage {
            lineage_id: identity.lineage_id.clone(),
            active_version_id: active_version_id.to_owned(),
            expected_active_path,
            retained_versions: vec![retained],
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn rollback(
    original: SwapError,
    activated: Option<&Path>,
    swap: &ReplacementSwap,
    retained_path: &Path,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    identity: &Identity,
    storage_path: &Path,
) -> SwapError {
    let mut failures = Vec::new();
    if let Some(active) = activated
        && let Err(error) = exclusive_rename(active, &swap.prepared_replacement)
    {
        failures.push(format!("cannot return replacement to staging: {error}"));
    }
    if swap.prepared_replacement.exists()
        && let Err(error) = exclusive_rename(retained_path, &swap.context.active_path)
    {
        failures.push(format!(
            "cannot restore the previous active release: {error}"
        ));
    }
    remove_empty_parents(created_parents);
    if !retained_path.exists()
        && let Err(error) = store.discard_prepared_payload(
            lock,
            &identity.lineage_id,
            &identity.retained_version_id,
            storage_path,
        )
    {
        failures.push(format!("cannot remove empty recovery container: {error}"));
    }
    if failures.is_empty() {
        original
    } else {
        SwapError::Rollback {
            original: Box::new(original),
            failures,
        }
    }
}

fn cleanup_before_first_move(
    original: SwapError,
    created_parents: Vec<PathBuf>,
    store: &RecoveryStore,
    lock: &crate::recovery::RecoveryLock,
    lineage_id: &str,
    version_id: &str,
    storage_path: &Path,
) -> SwapError {
    remove_empty_parents(created_parents);
    match store.discard_prepared_payload(lock, lineage_id, version_id, storage_path) {
        Ok(()) => original,
        Err(error) => SwapError::Rollback {
            original: Box::new(original),
            failures: vec![format!("cannot remove empty recovery container: {error}")],
        },
    }
}

fn validate_request(swap: &ReplacementSwap) -> Result<(), SwapError> {
    if swap.display_label.trim().is_empty() {
        return Err(SwapError::InvalidRequest(
            "retained release display label is empty".into(),
        ));
    }
    if swap.protected_until < swap.retained_at {
        return Err(SwapError::InvalidRequest(
            "protection deadline is before the retention time".into(),
        ));
    }
    let root = swap
        .context
        .library_root
        .canonicalize()
        .map_err(|source| RecoveryError::Io(swap.context.library_root.clone(), source))?;
    if root != swap.context.library_root {
        return Err(SwapError::UnsafePath(swap.context.library_root.clone()));
    }
    let prepared = swap
        .prepared_replacement
        .canonicalize()
        .map_err(|source| RecoveryError::Io(swap.prepared_replacement.clone(), source))?;
    if prepared == swap.context.active_path
        || prepared == swap.context.destination
        || !prepared.starts_with(&root)
        || prepared.starts_with(root.join(crate::recovery::RECOVERY_DIRECTORY))
    {
        return Err(SwapError::UnsafePath(swap.prepared_replacement.clone()));
    }
    let metadata = fs::symlink_metadata(&prepared)
        .map_err(|source| RecoveryError::Io(prepared.clone(), source))?;
    if !metadata.file_type().is_dir() {
        return Err(SwapError::UnsafePath(prepared));
    }
    let historical = swap
        .context
        .active_path
        .strip_prefix(&root)
        .map_err(|_| SwapError::UnsafePath(swap.context.active_path.clone()))?;
    if historical != swap.context.historical_path {
        return Err(SwapError::UnsafePath(swap.context.historical_path.clone()));
    }
    let destination = swap
        .context
        .destination
        .strip_prefix(&root)
        .map_err(|_| SwapError::UnsafePath(swap.context.destination.clone()))?;
    if destination.as_os_str().is_empty()
        || destination
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || destination
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == crate::recovery::RECOVERY_DIRECTORY)
    {
        return Err(SwapError::UnsafePath(swap.context.destination.clone()));
    }
    match fs::symlink_metadata(&swap.context.destination) {
        Ok(_) if swap.context.destination != swap.context.active_path => {
            return Err(SwapError::DestinationCollision(
                swap.context.destination.clone(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(RecoveryError::Io(swap.context.destination.clone(), source).into());
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum SwapError {
    InvalidRequest(String),
    UnsafePath(PathBuf),
    DestinationCollision(PathBuf),
    IdentityMismatch(PathBuf),
    Recovery(RecoveryError),
    Publication(PublicationError),
    Checkpoint(String),
    Rollback {
        original: Box<SwapError>,
        failures: Vec<String>,
    },
}

impl SwapError {
    pub fn rollback_incomplete(&self) -> bool {
        matches!(self, Self::Rollback { .. })
    }
}

impl From<RecoveryError> for SwapError {
    fn from(error: RecoveryError) -> Self {
        Self::Recovery(error)
    }
}

impl From<PublicationError> for SwapError {
    fn from(error: PublicationError) -> Self {
        Self::Publication(error)
    }
}

impl fmt::Display for SwapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(cause) => write!(formatter, "invalid replacement swap: {cause}"),
            Self::UnsafePath(path) => {
                write!(formatter, "unsafe replacement path: {}", path.display())
            }
            Self::DestinationCollision(path) => write!(
                formatter,
                "replacement destination is occupied by unrelated content: {}",
                path.display()
            ),
            Self::IdentityMismatch(path) => write!(
                formatter,
                "replacement identity does not match recovery history at {}",
                path.display()
            ),
            Self::Recovery(error) => write!(formatter, "{error}"),
            Self::Publication(error) => write!(formatter, "{error}"),
            Self::Checkpoint(cause) => write!(formatter, "replacement interrupted: {cause}"),
            Self::Rollback { original, failures } => write!(
                formatter,
                "{original}; rollback was incomplete: {}",
                failures.join("; ")
            ),
        }
    }
}

impl std::error::Error for SwapError {}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn same_path_swap_retains_old_release_and_commits_identity() {
        let fixture = Fixture::new(false);

        let report = swap_prepared(&fixture.swap).unwrap();

        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"new");
        assert_eq!(
            fs::read(report.retained_path.join("track.flac")).unwrap(),
            b"old"
        );
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let index = store.load_index().unwrap();
        assert_eq!(index.lineages.len(), 1);
        assert_eq!(index.lineages[0].lineage_id, report.lineage_id);
        assert_eq!(
            index.lineages[0].active_version_id,
            report.active_version_id
        );
        assert_eq!(index.lineages[0].retained_versions[0].protected_until, 200);
        assert_eq!(
            store.read_active_receipt(&fixture.active).unwrap(),
            ActiveReceipt::new(report.lineage_id, report.active_version_id)
        );
    }

    #[test]
    fn relocation_records_both_historical_and_new_active_paths() {
        let fixture = Fixture::new(true);
        let destination = fixture.swap.context.destination.clone();

        let report = swap_prepared(&fixture.swap).unwrap();

        assert!(!fixture.active.exists());
        assert_eq!(fs::read(destination.join("track.flac")).unwrap(), b"new");
        assert_eq!(
            fs::read(report.retained_path.join("track.flac")).unwrap(),
            b"old"
        );
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let lineage = &store.load_index().unwrap().lineages[0];
        assert_eq!(lineage.expected_active_path, Path::new("Artist/New Album"));
        assert_eq!(
            lineage.retained_versions[0].historical_path,
            Path::new("Artist/Old Album")
        );
    }

    #[test]
    fn later_swap_reuses_lineage_and_appends_history() {
        let mut fixture = Fixture::new(false);
        let first = swap_prepared(&fixture.swap).unwrap();
        fixture.prepared = fixture.library.join(".prepared-2");
        fs::create_dir(&fixture.prepared).unwrap();
        fs::write(fixture.prepared.join("track.flac"), b"newer").unwrap();
        fixture.swap.prepared_replacement = fixture.prepared.clone();
        fixture.swap.retained_at = 300;
        fixture.swap.protected_until = 400;

        let second = swap_prepared(&fixture.swap).unwrap();

        assert_eq!(second.lineage_id, first.lineage_id);
        assert_eq!(
            first.retained_path.parent().unwrap().parent(),
            second.retained_path.parent().unwrap().parent()
        );
        assert_ne!(first.retained_path, second.retained_path);
        assert!(
            first
                .retained_path
                .to_string_lossy()
                .contains("Artist — Old Album")
        );
        assert!(
            !first
                .retained_path
                .to_string_lossy()
                .contains(&first.lineage_id)
        );
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        let index = store.load_index().unwrap();
        assert_eq!(index.lineages.len(), 1);
        assert_eq!(index.lineages[0].retained_versions.len(), 2);
        assert_eq!(
            fs::read(fixture.active.join("track.flac")).unwrap(),
            b"newer"
        );
    }

    #[test]
    fn conflicting_active_receipt_stops_before_any_move() {
        let mut fixture = Fixture::new(false);
        swap_prepared(&fixture.swap).unwrap();
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
        fixture.prepared = fixture.library.join(".prepared-2");
        fs::create_dir(&fixture.prepared).unwrap();
        fs::write(fixture.prepared.join("track.flac"), b"newer").unwrap();
        fixture.swap.prepared_replacement = fixture.prepared.clone();

        let error = swap_prepared(&fixture.swap).unwrap_err();

        assert!(matches!(error, SwapError::IdentityMismatch(path) if path == fixture.active));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"new");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"newer"
        );
        assert_eq!(store.load_index().unwrap().lineages.len(), 1);
    }

    #[test]
    fn occupied_destination_is_refused_without_overwriting_either_release() {
        let fixture = Fixture::new(true);
        let destination = fixture.swap.context.destination.clone();
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("unrelated"), b"keep").unwrap();

        let error = swap_prepared(&fixture.swap).unwrap_err();

        assert!(matches!(error, SwapError::DestinationCollision(path) if path == destination));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"old");
        assert_eq!(fs::read(destination.join("unrelated")).unwrap(), b"keep");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn destination_created_after_first_move_survives_and_old_active_is_restored() {
        let fixture = Fixture::new(true);
        let destination = fixture.swap.context.destination.clone();

        let error = swap_prepared_with(&fixture.swap, |point| {
            if point == SwapPoint::ActiveRetained {
                fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
                fs::write(destination.join("unrelated"), b"keep")
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })
        .unwrap_err();

        assert!(error.to_string().contains("was not overwritten"));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"old");
        assert_eq!(fs::read(destination.join("unrelated")).unwrap(), b"keep");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn destination_cannot_escape_the_library_lexically() {
        let mut fixture = Fixture::new(true);
        fixture.swap.context.destination = fixture.library.join("../outside");

        let error = swap_prepared(&fixture.swap).unwrap_err();

        assert!(matches!(error, SwapError::UnsafePath(_)));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"old");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn failure_after_retaining_active_rolls_back_exactly() {
        let fixture = Fixture::new(true);

        let error = swap_prepared_with(&fixture.swap, |point| {
            (point != SwapPoint::ActiveRetained)
                .then_some(())
                .ok_or_else(|| "injected after first move".into())
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected after first move"));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"old");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"new"
        );
        assert!(!fixture.swap.context.destination.exists());
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        assert!(store.load_index().unwrap().lineages.is_empty());
    }

    #[test]
    fn failure_before_index_commit_reverses_both_moves() {
        let fixture = Fixture::new(true);

        let error = swap_prepared_with(&fixture.swap, |point| {
            (point != SwapPoint::BeforeIndexCommit)
                .then_some(())
                .ok_or_else(|| "injected before commit".into())
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected before commit"));
        assert_eq!(fs::read(fixture.active.join("track.flac")).unwrap(), b"old");
        assert_eq!(
            fs::read(fixture.prepared.join("track.flac")).unwrap(),
            b"new"
        );
        assert!(!fixture.swap.context.destination.exists());
        let store = RecoveryStore::open_existing(&fixture.library)
            .unwrap()
            .unwrap();
        assert!(store.load_index().unwrap().lineages.is_empty());
        assert!(!fixture.active.join(ACTIVE_RECEIPT).exists());
    }

    struct Fixture {
        _temporary: TempDir,
        library: PathBuf,
        active: PathBuf,
        prepared: PathBuf,
        swap: ReplacementSwap,
    }

    impl Fixture {
        fn new(relocate: bool) -> Self {
            let temporary = TempDir::new().unwrap();
            let library = temporary.path().join("library");
            let active = library.join("Artist/Old Album");
            let prepared = library.join(".prepared");
            fs::create_dir_all(&active).unwrap();
            fs::create_dir(&prepared).unwrap();
            fs::write(active.join("track.flac"), b"old").unwrap();
            fs::write(prepared.join("track.flac"), b"new").unwrap();
            let destination = if relocate {
                library.join("Artist/New Album")
            } else {
                active.clone()
            };
            let swap = ReplacementSwap {
                context: ReplacementContext {
                    library_root: library.clone(),
                    active_path: active.clone(),
                    historical_path: PathBuf::from("Artist/Old Album"),
                    destination,
                },
                prepared_replacement: prepared.clone(),
                display_label: "Artist — Old Album".into(),
                retained_at: 100,
                protected_until: 200,
            };
            Self {
                _temporary: temporary,
                library,
                active,
                prepared,
                swap,
            }
        }
    }
}
