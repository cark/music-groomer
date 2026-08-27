use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use music_groomer::artwork_viewer::SystemArtworkViewer;
use music_groomer::config::AppConfig;
use music_groomer::demo::{self, DemoScenario};
use music_groomer::guided_matching;
use music_groomer::inspection_ui;
use music_groomer::matching_ui::{MetadataSelection, coherent_existing_metadata};
use music_groomer::provider::{
    CoverArtArchive, MusicBrainzProvider, ProviderCache, source_inspection,
};
use music_groomer::source::SourceInspector;
use music_groomer::terminal::StdioInteraction;

mod cli;

use cli::{CacheAction, Cli, CliCommand};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("music-groomer: {message}");
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
                println!();
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
    println!("music-groomer provider cache");
    println!("  Location: {}", status.location.display());
    println!(
        "  Usage: {} / {}",
        byte_count(status.total_bytes),
        byte_count(status.max_bytes)
    );
    println!(
        "  Metadata: {} fresh, {} stale",
        status.fresh_metadata, status.stale_metadata
    );
    println!(
        "  Artwork: {} images, {}; {} confirmed absent",
        status.artwork_entries,
        byte_count(status.artwork_bytes),
        status.confirmed_artwork_absences
    );
    println!("  Damaged entries: {}", status.damaged_entries);
    Ok(())
}

fn clear_cache(cache: &ProviderCache) -> Result<(), String> {
    let status = cache
        .status(std::time::SystemTime::now())
        .map_err(|error| error.to_string())?;
    println!("Clear only music-groomer's provider cache?");
    println!("  Path: {}", status.location.display());
    println!("  Current size: {}", byte_count(status.total_bytes));
    print!("Continue? [y/N]: ");
    use std::io::Write as _;
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        println!("Cache left unchanged.");
        return Ok(());
    }
    cache.clear().map_err(|error| error.to_string())?;
    println!("Provider cache cleared.");
    Ok(())
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
    let stdin = io::stdin();
    let stdout = io::stdout();
    let styling = stdout.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut interaction = StdioInteraction::new(stdin.lock(), stdout.lock(), styling);
    demo::run(&mut interaction, scenario, output.as_deref())
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
    let stdin = io::stdin();
    let stdout = io::stdout();
    let styling = stdout.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut interaction = StdioInteraction::new(stdin.lock(), stdout.lock(), styling);
    if inspection.is_blocked() {
        inspection_ui::run(&mut interaction, &inspection)
            .map_err(|error| format!("terminal interaction failed: {error}"))?;
        Err("inspection found blocking problems; the source remains untouched".to_owned())
    } else {
        inspection_ui::run_before_matching(&mut interaction, &inspection)
            .map_err(|error| format!("terminal interaction failed: {error}"))?;
        let config = AppConfig::load().map_err(|error| error.to_string())?;
        let cache = provider_cache(&config, cache_directory)?;
        let mut viewer = SystemArtworkViewer::new();
        let result = guided_matching::run(
            &mut interaction,
            &inspection,
            offline,
            MusicBrainzProvider::new(),
            CoverArtArchive::new(),
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
