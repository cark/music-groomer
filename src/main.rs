#![deny(clippy::disallowed_macros)]

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use music_groomer::artwork_viewer::SystemArtworkViewer;
use music_groomer::config::AppConfig;
use music_groomer::demo::{self, DemoScenario};
use music_groomer::fingerprint::FpcalcFingerprinter;
use music_groomer::guided_matching;
use music_groomer::inspection_ui;
use music_groomer::matching_ui::{MetadataSelection, coherent_existing_metadata};
use music_groomer::provider::{
    AcoustId, CoverArtArchive, MusicBrainzProvider, ProviderCache, source_inspection,
};
use music_groomer::source::SourceInspector;
use music_groomer::terminal::{Interaction, StdioInteraction, UiLine};

mod cli;

use cli::{CacheAction, Cli, CliCommand};

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
    let cache_directory = arguments.cache_dir;
    match arguments.command {
        Some(CliCommand::Demo { scenario, output }) => {
            if cache_directory.is_some() {
                return Err("--cache-dir does not apply to the simulated demo".to_owned());
            }
            run_demo(scenario.map(|value| value.name()), output)
        }
        Some(CliCommand::Cache { action }) => run_cache(action, cache_directory),
        None => match arguments.source {
            Some(source) => run_inspection(source, arguments.offline, cache_directory),
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
        interaction.prompt(UiLine::menu_prompt("Continue? [y/N]: "))
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

fn byte_count(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn run_demo(scenario: Option<&str>, output: Option<PathBuf>) -> Result<(), String> {
    let scenario = scenario.map(|value| {
        DemoScenario::parse(value).expect("Clap accepts only supported demo scenario names")
    });
    with_stdio_interaction(|interaction| demo::run(interaction, scenario, output.as_deref()))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn run_inspection(
    source: PathBuf,
    offline: bool,
    cache_directory: Option<PathBuf>,
) -> Result<(), String> {
    let inspection = SourceInspector::default()
        .inspect(&source)
        .map_err(|error| error.to_string())?;
    with_stdio_interaction(|interaction| {
        if inspection.is_blocked() {
            inspection_ui::run(interaction, &inspection)
                .map_err(|error| format!("terminal interaction failed: {error}"))?;
            Err("inspection found blocking problems; the source remains untouched".to_owned())
        } else {
            inspection_ui::run_before_matching(interaction, &inspection)
                .map_err(|error| format!("terminal interaction failed: {error}"))?;
            let config = AppConfig::load().map_err(|error| error.to_string())?;
            let cache = provider_cache(&config, cache_directory)?;
            let mut viewer = SystemArtworkViewer::new();
            let mut fingerprinter = FpcalcFingerprinter::default();
            let mut acoustid = AcoustId::new();
            let result = guided_matching::run_with_identification(
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
            }
            Ok(())
        }
    })
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
