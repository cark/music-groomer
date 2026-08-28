use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::AppConfig;
use crate::recovery::{RecoveryStore, RetainedVersion, retained_time_label};
use crate::replacement::{RestoreRequest, restore};
use crate::terminal::{Interaction, UiLine, byte_count};

#[derive(Debug)]
pub enum GuidedRecoveryError {
    Io(io::Error),
    Operation(String),
}

impl fmt::Display for GuidedRecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "terminal interaction failed: {error}"),
            Self::Operation(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for GuidedRecoveryError {}

impl From<io::Error> for GuidedRecoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
struct RecoveryEntry {
    lineage_id: String,
    active_path: PathBuf,
    version: RetainedVersion,
}

pub fn run(
    interaction: &mut impl Interaction,
    library_root: Option<&Path>,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<(), GuidedRecoveryError> {
    loop {
        let now = now()?;
        let entries = load_entries(library_root)?;
        render_overview(interaction, library_root, config, &entries, now)?;
        let answer = interaction.prompt(UiLine::prompt(if entries.is_empty() {
            "Choose [p] Preferences or [q] Done: "
        } else {
            "Choose a retained copy, [p] Preferences, or [q] Done: "
        }))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "p" | "preferences" => edit_preferences(interaction, config, config_path)?,
            "q" | "quit" | "done" | "" => return Ok(()),
            selection => match selection.parse::<usize>() {
                Ok(number) if number > 0 && number <= entries.len() => {
                    show_entry(
                        interaction,
                        library_root.expect("entries require a configured library"),
                        config,
                        &entries[number - 1],
                        now,
                    )?;
                }
                _ => interaction.error("Choose one of the displayed numbers, p, or q.")?,
            },
        }
    }
}

fn load_entries(library_root: Option<&Path>) -> Result<Vec<RecoveryEntry>, GuidedRecoveryError> {
    let Some(library_root) = library_root else {
        return Ok(Vec::new());
    };
    let Some(store) = RecoveryStore::open_existing(library_root)
        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?
    else {
        return Ok(Vec::new());
    };
    let index = store
        .load_index()
        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?;
    let mut entries = Vec::new();
    for lineage in index.lineages {
        for mut version in lineage.retained_versions {
            version.size_bytes = store
                .retained_size(
                    &lineage.lineage_id,
                    &version.version_id,
                    &version.storage_path,
                )
                .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?;
            entries.push(RecoveryEntry {
                lineage_id: lineage.lineage_id.clone(),
                active_path: library_root.join(&lineage.expected_active_path),
                version,
            });
        }
    }
    entries.sort_by(|left, right| {
        right
            .version
            .retained_at
            .cmp(&left.version.retained_at)
            .then_with(|| left.version.display_label.cmp(&right.version.display_label))
            .then_with(|| left.version.version_id.cmp(&right.version.version_id))
    });
    Ok(entries)
}

fn render_overview(
    interaction: &mut impl Interaction,
    library_root: Option<&Path>,
    config: &AppConfig,
    entries: &[RecoveryEntry],
    now: u64,
) -> Result<(), GuidedRecoveryError> {
    interaction.section_heading("Recovery")?;
    if let Some(library_root) = library_root {
        interaction.path_field("Library", library_root.display().to_string())?;
    } else {
        interaction.field("Library", "not configured")?;
    }
    interaction.field(
        "Grace period",
        format!("{} days", recovery_grace_days(config)?),
    )?;
    interaction.field("Storage limit", byte_count(recovery_max_bytes(config)?))?;
    interaction.section_heading("Retained copies")?;
    if entries.is_empty() {
        interaction.prose("  No retained copies are available.")?;
        return Ok(());
    }
    for (position, entry) in entries.iter().enumerate() {
        interaction.present(UiLine::menu_item(
            (position + 1).to_string(),
            &entry.version.display_label,
        ))?;
        interaction.path_field(
            "  Historical path",
            library_root
                .expect("entries require a configured library")
                .join(&entry.version.historical_path)
                .display()
                .to_string(),
        )?;
        interaction.field("  Retained", time_label(entry.version.retained_at))?;
        interaction.field("  Size", byte_count(entry.version.size_bytes))?;
        interaction.field("  Status", protection_label(&entry.version, now))?;
    }
    Ok(())
}

fn show_entry(
    interaction: &mut impl Interaction,
    library_root: &Path,
    config: &AppConfig,
    entry: &RecoveryEntry,
    snapshot_now: u64,
) -> Result<(), GuidedRecoveryError> {
    loop {
        interaction.section_heading("Retained copy")?;
        interaction.field("Name", &entry.version.display_label)?;
        interaction.path_field(
            "Historical path",
            library_root
                .join(&entry.version.historical_path)
                .display()
                .to_string(),
        )?;
        interaction.path_field(
            "Current active path",
            entry.active_path.display().to_string(),
        )?;
        interaction.field("Retained", time_label(entry.version.retained_at))?;
        interaction.field("Size", byte_count(entry.version.size_bytes))?;
        interaction.field("Status", protection_label(&entry.version, snapshot_now))?;
        let answer = interaction.prompt(UiLine::prompt(
            "Choose [r] Restore, [d] Remove, or [b] Back: ",
        ))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "r" | "restore" => {
                let grace_days = recovery_grace_days(config)?;
                let displayed_deadline = protection_deadline(now()?, grace_days)?;
                if confirm_restore(
                    interaction,
                    library_root,
                    entry,
                    grace_days,
                    displayed_deadline,
                )? {
                    let retained_at = now()?;
                    let protected_until = protection_deadline(retained_at, grace_days)?;
                    let report = restore(&RestoreRequest {
                        library_root: library_root.to_owned(),
                        lineage_id: entry.lineage_id.clone(),
                        version_id: entry.version.version_id.clone(),
                        retained_at,
                        protected_until,
                    })
                    .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?;
                    interaction.success(format!(
                        "✓ Restored {} to {}.",
                        entry.version.display_label,
                        report.active_path.display()
                    ))?;
                    interaction.prose(format!(
                        "  The displaced active release is safely stashed as {} for at least {grace_days} days, until {}.",
                        report.displaced_display_label,
                        time_label(report.displaced_protected_until)
                    ))?;
                    if let Some(warning) = report.cleanup_warning {
                        interaction
                            .warning(format!("Restore cleanup was incomplete: {warning}"))?;
                    }
                    return Ok(());
                }
            }
            "d" | "delete" | "remove" => {
                if confirm_remove(interaction, library_root, entry)? {
                    let store = RecoveryStore::open_existing(library_root)
                        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?
                        .ok_or_else(|| {
                            GuidedRecoveryError::Operation(
                                "the recovery store disappeared before removal".into(),
                            )
                        })?;
                    let report = store
                        .remove_retained(&entry.lineage_id, &entry.version.version_id)
                        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))?;
                    interaction.success(format!(
                        "✓ Removed {} · freed {}.",
                        report.display_label,
                        byte_count(report.size_bytes)
                    ))?;
                    if let Some(warning) = report.cleanup_warning {
                        interaction
                            .warning(format!("Removal cleanup was incomplete: {warning}"))?;
                    }
                    return Ok(());
                }
            }
            "b" | "back" | "" => return Ok(()),
            _ => interaction.error("Choose r, d, or b.")?,
        }
    }
}

fn confirm_restore(
    interaction: &mut impl Interaction,
    library_root: &Path,
    entry: &RecoveryEntry,
    grace_days: u64,
    protected_until: u64,
) -> io::Result<bool> {
    interaction.section_heading("RESTORE RETAINED COPY")?;
    interaction.warning(
        "Warning: the retained copy will become active and the current active release will be safely stashed.",
    )?;
    interaction.field("Name", &entry.version.display_label)?;
    interaction.path_field(
        "Current active path",
        entry.active_path.display().to_string(),
    )?;
    interaction.path_field(
        "Restore to",
        library_root
            .join(&entry.version.historical_path)
            .display()
            .to_string(),
    )?;
    interaction.field(
        "Current release protection",
        format!("{grace_days} days, until {}", time_label(protected_until)),
    )?;
    loop {
        let answer = interaction.prompt(UiLine::confirmation_prompt("Restore it? [y/N]: "))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer y or n; Enter means no.")?,
        }
    }
}

fn confirm_remove(
    interaction: &mut impl Interaction,
    library_root: &Path,
    entry: &RecoveryEntry,
) -> io::Result<bool> {
    interaction.section_heading("REMOVE RETAINED COPY")?;
    interaction.warning("Warning: this retained copy will no longer be restorable.")?;
    interaction.field("Name", &entry.version.display_label)?;
    interaction.path_field(
        "Historical path",
        library_root
            .join(&entry.version.historical_path)
            .display()
            .to_string(),
    )?;
    interaction.field("Retained", time_label(entry.version.retained_at))?;
    interaction.field("Size", byte_count(entry.version.size_bytes))?;
    loop {
        let answer = interaction.prompt(UiLine::confirmation_prompt("Remove it? [y/N]: "))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => interaction.error("Please answer y or n; Enter means no.")?,
        }
    }
}

fn edit_preferences(
    interaction: &mut impl Interaction,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<(), GuidedRecoveryError> {
    loop {
        interaction.section_heading("Recovery preferences")?;
        interaction.field(
            "Grace period",
            format!("{} days", recovery_grace_days(config)?),
        )?;
        interaction.field("Storage limit", byte_count(recovery_max_bytes(config)?))?;
        let answer = interaction.prompt(UiLine::prompt(
            "Choose [g] Grace period, [s] Storage limit, or [b] Back: ",
        ))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "g" | "grace" => edit_grace(interaction, config, config_path)?,
            "s" | "storage" => edit_storage(interaction, config, config_path)?,
            "b" | "back" | "" => return Ok(()),
            _ => interaction.error("Choose g, s, or b.")?,
        }
    }
}

fn edit_grace(
    interaction: &mut impl Interaction,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<(), GuidedRecoveryError> {
    let answer = interaction.prompt(UiLine::prompt(format!(
        "Grace period in whole days [{}] (Enter to cancel): ",
        recovery_grace_days(config)?
    )))?;
    if answer.trim().is_empty() {
        return Ok(());
    }
    let value = match answer.trim().parse::<u64>() {
        Ok(value) if value > 0 => value,
        _ => {
            interaction.error("Enter a positive whole number of days.")?;
            return Ok(());
        }
    };
    save_preference(interaction, config, config_path, |candidate| {
        candidate.recovery_grace_days = Some(value);
    })
}

fn edit_storage(
    interaction: &mut impl Interaction,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<(), GuidedRecoveryError> {
    let current = recovery_max_bytes(config)? / (1024 * 1024);
    let answer = interaction.prompt(UiLine::prompt(format!(
        "Storage limit in whole MiB [{current}] (Enter to cancel): "
    )))?;
    if answer.trim().is_empty() {
        return Ok(());
    }
    let value = match answer.trim().parse::<u64>() {
        Ok(value) if value > 0 => value,
        _ => {
            interaction.error("Enter a positive whole number of MiB.")?;
            return Ok(());
        }
    };
    save_preference(interaction, config, config_path, |candidate| {
        candidate.recovery_max_mib = Some(value);
    })
}

fn save_preference(
    interaction: &mut impl Interaction,
    config: &mut AppConfig,
    config_path: &Path,
    update: impl FnOnce(&mut AppConfig),
) -> Result<(), GuidedRecoveryError> {
    let mut candidate = config.clone();
    update(&mut candidate);
    recovery_grace_days(&candidate)?;
    recovery_max_bytes(&candidate)?;
    match candidate.save_to(config_path) {
        Ok(()) => {
            *config = candidate;
            interaction.success("Recovery preferences saved.")?;
        }
        Err(error) => interaction.error(format!("Could not save preferences: {error}"))?,
    }
    Ok(())
}

fn protection_label(version: &RetainedVersion, now: u64) -> String {
    if version.protected_until > now {
        format!("protected until {}", time_label(version.protected_until))
    } else {
        "eligible for automatic cleanup".into()
    }
}

fn recovery_grace_days(config: &AppConfig) -> Result<u64, GuidedRecoveryError> {
    config
        .recovery_grace_days()
        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))
}

fn recovery_max_bytes(config: &AppConfig) -> Result<u64, GuidedRecoveryError> {
    config
        .recovery_max_bytes()
        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))
}

fn time_label(timestamp: u64) -> String {
    retained_time_label(timestamp).unwrap_or_else(|_| "unknown time".into())
}

fn protection_deadline(now: u64, grace_days: u64) -> Result<u64, GuidedRecoveryError> {
    grace_days
        .checked_mul(24 * 60 * 60)
        .and_then(|seconds| now.checked_add(seconds))
        .ok_or_else(|| {
            GuidedRecoveryError::Operation(
                "the configured recovery grace period is too large to schedule safely".into(),
            )
        })
}

fn now() -> Result<u64, GuidedRecoveryError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| GuidedRecoveryError::Operation(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::recovery::{
        ActiveReceipt, RecoveryIndex, ReleaseLineage, new_lineage_id, new_version_id,
    };

    struct ScriptedInteraction {
        answers: VecDeque<String>,
        transcript: String,
    }

    impl ScriptedInteraction {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|answer| (*answer).to_owned()).collect(),
                transcript: String::new(),
            }
        }
    }

    impl Interaction for ScriptedInteraction {
        fn present(&mut self, line: UiLine) -> io::Result<()> {
            self.transcript.push_str(&line.plain_text());
            self.transcript.push('\n');
            Ok(())
        }

        fn prompt(&mut self, prompt: UiLine) -> io::Result<String> {
            self.transcript.push_str(&prompt.plain_text());
            Ok(self.answers.pop_front().unwrap_or_default())
        }
    }

    #[test]
    fn preferences_work_without_creating_a_recovery_store() {
        let temporary = TempDir::new().unwrap();
        let library = temporary.path().join("library");
        let config_path = temporary.path().join("config/config.toml");
        fs::create_dir(&library).unwrap();
        let mut config = AppConfig::default();
        let mut interaction = ScriptedInteraction::new(&["p", "g", "45", "b", "q"]);

        run(&mut interaction, Some(&library), &mut config, &config_path).unwrap();

        assert_eq!(config.recovery_grace_days, Some(45));
        assert_eq!(AppConfig::load_from(&config_path).unwrap(), config);
        assert!(!library.join(crate::recovery::RECOVERY_DIRECTORY).exists());
        assert!(interaction.transcript.contains("No retained copies"));
        assert!(
            interaction
                .transcript
                .contains("Recovery preferences saved")
        );
    }

    #[test]
    fn preference_changes_do_not_rewrite_existing_protection_deadlines() {
        let fixture = Fixture::new();
        let mut config = AppConfig::default();
        let mut interaction = ScriptedInteraction::new(&["p", "g", "7", "b", "q"]);

        run(
            &mut interaction,
            Some(&fixture.library),
            &mut config,
            &fixture.config_path,
        )
        .unwrap();

        assert_eq!(config.recovery_grace_days, Some(7));
        assert_eq!(
            fixture.store().load_index().unwrap().lineages[0].retained_versions[0].protected_until,
            u64::MAX
        );
    }

    #[test]
    fn removal_defaults_to_no_and_confirmed_removal_deletes_only_the_retained_copy() {
        let declined = Fixture::new();
        let mut interaction = ScriptedInteraction::new(&["1", "d", "", "b", "q"]);
        run(
            &mut interaction,
            Some(&declined.library),
            &mut AppConfig::default(),
            &declined.config_path,
        )
        .unwrap();
        assert_eq!(
            declined.store().load_index().unwrap().lineages[0]
                .retained_versions
                .len(),
            1
        );
        assert!(declined.retained_payload().exists());

        let confirmed = Fixture::new();
        let mut interaction = ScriptedInteraction::new(&["1", "d", "y", "q"]);
        run(
            &mut interaction,
            Some(&confirmed.library),
            &mut AppConfig::default(),
            &confirmed.config_path,
        )
        .unwrap();
        assert!(
            confirmed.store().load_index().unwrap().lineages[0]
                .retained_versions
                .is_empty()
        );
        assert!(!confirmed.retained_payload().exists());
        assert_eq!(
            fs::read(confirmed.active.join("track")).unwrap(),
            b"current"
        );
        assert!(
            interaction
                .transcript
                .contains("Removed Artist — Old Album")
        );
    }

    #[test]
    fn restore_defaults_to_no_and_confirmed_restore_resets_displaced_protection() {
        let declined = Fixture::new();
        let mut interaction = ScriptedInteraction::new(&["1", "r", "", "b", "q"]);
        run(
            &mut interaction,
            Some(&declined.library),
            &mut AppConfig::default(),
            &declined.config_path,
        )
        .unwrap();
        assert_eq!(fs::read(declined.active.join("track")).unwrap(), b"current");
        assert_eq!(
            fs::read(declined.retained_payload().join("track")).unwrap(),
            b"old"
        );
        assert!(interaction.transcript.contains("Restore it? [y/N]"));

        let confirmed = Fixture::new();
        let before = now().unwrap();
        let mut interaction = ScriptedInteraction::new(&["1", "r", "y", "q"]);
        run(
            &mut interaction,
            Some(&confirmed.library),
            &mut AppConfig::default(),
            &confirmed.config_path,
        )
        .unwrap();
        let index = confirmed.store().load_index().unwrap();
        let lineage = &index.lineages[0];
        assert_eq!(lineage.active_version_id, confirmed.selected_id);
        assert_eq!(fs::read(confirmed.active.join("track")).unwrap(), b"old");
        assert_eq!(lineage.retained_versions.len(), 1);
        assert!(lineage.retained_versions[0].retained_at >= before);
        assert_eq!(
            lineage.retained_versions[0].protected_until,
            lineage.retained_versions[0].retained_at + 30 * 24 * 60 * 60
        );
        assert!(interaction.transcript.contains("Current active path"));
        assert!(
            interaction
                .transcript
                .contains("safely stashed as Artist — Album")
        );
    }

    struct Fixture {
        _temporary: TempDir,
        library: PathBuf,
        active: PathBuf,
        selected_id: String,
        storage_path: PathBuf,
        config_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let library = temporary.path().join("library");
            let active = library.join("Artist/Album");
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
                    historical_path: PathBuf::from("Artist/Album"),
                    display_label: "Artist — Old Album".into(),
                    retained_at: 10,
                    protected_until: u64::MAX,
                    size_bytes: 3,
                    storage_path: prepared.storage_path.clone(),
                }],
            });
            store.save_index(&lock, &index).unwrap();
            store
                .write_active_receipt(&lock, &active, &ActiveReceipt::new(&lineage_id, current_id))
                .unwrap();
            Self {
                config_path: temporary.path().join("config/config.toml"),
                _temporary: temporary,
                library,
                active,
                selected_id,
                storage_path: prepared.storage_path,
            }
        }

        fn store(&self) -> RecoveryStore {
            RecoveryStore::open_existing(&self.library)
                .unwrap()
                .unwrap()
        }

        fn retained_payload(&self) -> PathBuf {
            self.store()
                .retained_payload_path(&self.storage_path)
                .unwrap()
        }
    }
}
