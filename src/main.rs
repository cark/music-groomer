#![deny(clippy::disallowed_macros)]

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use music_groomer::artwork_viewer::SystemArtworkViewer;
use music_groomer::config::AppConfig;
use music_groomer::fingerprint::FpcalcFingerprinter;
use music_groomer::guided_matching;
use music_groomer::inspection_ui;
use music_groomer::matching_ui::{MetadataSelection, coherent_existing_metadata};
use music_groomer::provider::{
    AcoustId, CoverArtArchive, MusicBrainzProvider, ProviderCache, source_inspection,
};
use music_groomer::recovery::run_maintenance;
use music_groomer::recovery_ui::render_maintenance;
use music_groomer::source::SourceInspector;
use music_groomer::terminal::{Interaction, StdioInteraction, UiLine, byte_count};

mod cli;
mod diagnostics;

use cli::{CacheAction, Cli, CliCommand, RecoveryAction};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let diagnostic = format!("music-groomer: {message}");
            let render_failed = {
                let stdin = io::stdin();
                let stderr = io::stderr();
                let styling = stderr.is_terminal() && std::env::var_os("NO_COLOR").is_none();
                let mut interaction = StdioInteraction::new(stdin.lock(), stderr.lock(), styling);
                interaction.error(&diagnostic).is_err()
            };
            if render_failed {
                use std::io::Write as _;
                let _ = io::stderr().write_all(diagnostic.as_bytes());
                let _ = io::stderr().write_all(b"\n");
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = Cli::parse();
    let diagnostics = arguments
        .diagnostics
        .map(|scope| diagnostics::initialize(scope.includes_audio_libraries()))
        .transpose()
        .map_err(|error| error.to_string())?;
    let cache_directory = arguments.cache_dir;
    match arguments.command {
        Some(CliCommand::Cache { action }) => run_cache(action, cache_directory),
        Some(CliCommand::Recovery { action }) => run_recovery(action),
        None => match arguments.source {
            Some(source) => run_inspection(
                source,
                arguments.offline,
                cache_directory,
                arguments.output,
                diagnostics.as_deref(),
            ),
            None => {
                Cli::command()
                    .print_help()
                    .map_err(|error| error.to_string())?;
                use std::io::Write as _;
                io::stdout()
                    .write_all(b"\n")
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
        },
    }
}

fn run_recovery(action: RecoveryAction) -> Result<(), String> {
    match action {
        RecoveryAction::Maintain => {
            let config = AppConfig::load().map_err(|error| error.to_string())?;
            let configured = config.destination.as_ref().ok_or_else(|| {
                "no destination library is configured; choose and save one in a grooming session first"
                    .to_owned()
            })?;
            let library = configured.canonicalize().map_err(|error| {
                format!(
                    "cannot use configured destination library {}: {error}",
                    configured.display()
                )
            })?;
            let max_bytes = config
                .recovery_max_bytes()
                .map_err(|error| error.to_string())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_secs();
            let report = run_maintenance(&library, max_bytes, now)
                .map_err(|error| format!("recovery maintenance failed: {error}"))?;
            with_stdio_interaction(|interaction| {
                render_maintenance(interaction, &report, max_bytes, true, Some(&library))
            })
            .map_err(|error| error.to_string())
        }
    }
}

fn run_cache(action: Option<CacheAction>, cache_directory: Option<PathBuf>) -> Result<(), String> {
    let config = AppConfig::load().map_err(|error| error.to_string())?;
    let cache = provider_cache(&config, cache_directory)?;
    match action {
        None | Some(CacheAction::Status) => show_cache_status(&cache),
        Some(CacheAction::Clear) => clear_cache(&cache),
    }
}

fn show_cache_status(cache: &ProviderCache) -> Result<(), String> {
    let status = cache
        .status(std::time::SystemTime::now())
        .map_err(|error| error.to_string())?;
    with_stdio_interaction(|interaction| {
        interaction.heading("music-groomer provider cache")?;
        interaction.path_field("Location", status.location.display().to_string())?;
        interaction.field(
            "Usage",
            format!(
                "{} / {}",
                byte_count(status.total_bytes),
                byte_count(status.max_bytes)
            ),
        )?;
        interaction.field(
            "Metadata",
            format!(
                "{} fresh, {} stale",
                status.fresh_metadata, status.stale_metadata
            ),
        )?;
        interaction.field(
            "Artwork",
            format!(
                "{} images, {}; {} confirmed absent",
                status.artwork_entries,
                byte_count(status.artwork_bytes),
                status.confirmed_artwork_absences
            ),
        )?;
        interaction.field(
            "AcoustID",
            format!(
                "{} fresh, {} stale, {} cached no-match; {}",
                status.fresh_acoustid,
                status.stale_acoustid,
                status.acoustid_no_matches,
                byte_count(status.acoustid_bytes)
            ),
        )?;
        interaction.field("Obsolete entries", status.obsolete_entries.to_string())?;
        interaction.field("Damaged entries", status.damaged_entries.to_string())
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn clear_cache(cache: &ProviderCache) -> Result<(), String> {
    let status = cache
        .status(std::time::SystemTime::now())
        .map_err(|error| error.to_string())?;
    let answer = with_stdio_interaction(|interaction| {
        interaction.heading("Clear only music-groomer's provider cache?")?;
        interaction.path_field("Path", status.location.display().to_string())?;
        interaction.field("Current size", byte_count(status.total_bytes))?;
        interaction.prompt(UiLine::confirmation_prompt("Continue? [y/N]: "))
    })
    .map_err(|error| error.to_string())?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        with_stdio_interaction(|interaction| interaction.prose("Cache left unchanged."))
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    cache.clear().map_err(|error| error.to_string())?;
    with_stdio_interaction(|interaction| interaction.success("Provider cache cleared."))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn with_stdio_interaction<T, E>(
    action: impl FnOnce(&mut StdioInteraction<io::StdinLock<'_>, io::Stdout>) -> Result<T, E>,
) -> Result<T, E> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let interactive = stdout.is_terminal();
    let styling = interactive && std::env::var_os("NO_COLOR").is_none();
    let mut interaction = if interactive {
        StdioInteraction::for_terminal(stdin.lock(), stdout, styling)
    } else {
        StdioInteraction::new(stdin.lock(), stdout, styling)
    };
    action(&mut interaction)
}

fn run_inspection(
    source: PathBuf,
    offline: bool,
    cache_directory: Option<PathBuf>,
    output: Option<PathBuf>,
    diagnostics: Option<&std::path::Path>,
) -> Result<(), String> {
    with_stdio_interaction(|interaction| {
        if let Some(path) = diagnostics {
            interaction
                .path_field("Diagnostics", path.display().to_string())
                .map_err(|error| error.to_string())?;
        }
        interaction
            .status(UiLine::prose("Inspecting source..."))
            .map_err(|error| error.to_string())?;
        let mut progress = InteractionInspectionProgress {
            interaction,
            root: source
                .is_dir()
                .then_some(source.as_path())
                .or_else(|| source.parent())
                .unwrap_or_else(|| std::path::Path::new(""))
                .to_owned(),
        };
        let inspection = SourceInspector::default()
            .inspect_with_progress(&source, &mut progress)
            .map_err(|error| error.to_string())?;
        let interaction = progress.interaction;
        if inspection.is_blocked() {
            inspection_ui::run(interaction, &inspection)
                .map_err(|error| format!("terminal interaction failed: {error}"))?;
            Err("inspection found blocking problems; the source remains untouched".to_owned())
        } else {
            inspection_ui::run_before_matching(interaction, &inspection)
                .map_err(|error| format!("terminal interaction failed: {error}"))?;
            let mut config = AppConfig::load().map_err(|error| error.to_string())?;
            let cache = provider_cache(&config, cache_directory)?;
            let mut viewer = SystemArtworkViewer::new();
            let mut fingerprinter = FpcalcFingerprinter::default();
            let mut acoustid = AcoustId::new();
            let mut selected_plan = None;
            let result = guided_matching::run_with_identification_until(
                interaction,
                &inspection,
                offline,
                guided_matching::GuidedProviders::new(
                    MusicBrainzProvider::new(),
                    CoverArtArchive::new(),
                    &mut fingerprinter,
                    &mut acoustid,
                ),
                cache,
                &mut viewer,
                |interaction, matched| {
                    selected_plan = music_groomer::guided_apply::choose_initial_destination(
                        interaction,
                        &inspection,
                        matched,
                        &mut config,
                        output.as_deref(),
                    )?;
                    Ok(selected_plan.is_some())
                },
            )
            .map_err(|error| format!("terminal interaction failed: {error}"))?;
            if result.metadata == MetadataSelection::Cancelled {
                let (domain_inspection, _) = source_inspection(&inspection);
                if coherent_existing_metadata(&domain_inspection).is_err() {
                    return Err(
                        "no reliable metadata result is available; the source remains untouched"
                            .to_owned(),
                    );
                }
                return Ok(());
            }
            let plan = selected_plan.expect("metadata preview advances only with a destination");
            music_groomer::guided_apply::run_with_plan(
                interaction,
                &inspection,
                result,
                config,
                plan,
                &mut viewer,
            )
            .map_err(|error| error.to_string())
        }
    })
}

struct InteractionInspectionProgress<'a, I> {
    interaction: &'a mut I,
    root: PathBuf,
}

impl<I: Interaction> music_groomer::source::InspectionProgress
    for InteractionInspectionProgress<'_, I>
{
    fn inspecting_file(
        &mut self,
        path: &std::path::Path,
        number: usize,
        _bytes: u64,
    ) -> Result<(), String> {
        let shown = path.strip_prefix(&self.root).unwrap_or(path);
        self.interaction
            .status(UiLine::prose(format!(
                "Reading file {number}: {}",
                shown.display()
            )))
            .map_err(|error| error.to_string())
    }
}

fn provider_cache(
    config: &AppConfig,
    cache_directory: Option<PathBuf>,
) -> Result<ProviderCache, String> {
    let max_bytes = config
        .cache_max_bytes()
        .map_err(|error| error.to_string())?;
    match cache_directory {
        Some(directory) => Ok(ProviderCache::new(directory, max_bytes)),
        None => ProviderCache::platform_default(Some(max_bytes)).map_err(|error| error.to_string()),
    }
}
