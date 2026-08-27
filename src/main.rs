use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

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
    let mut arguments = env::args().skip(1);
    let Some(command) = arguments.next() else {
        print_help();
        return Ok(());
    };
    if matches!(command.as_str(), "-h" | "--help" | "help") {
        print_help();
        return Ok(());
    }
    if command == "demo" {
        return run_demo(arguments);
    }
    if command == "cache" {
        return run_cache(arguments);
    }
    let (source, offline) = parse_source(command, arguments)?;
    run_inspection(source, offline)
}

fn parse_source(
    first: String,
    mut arguments: impl Iterator<Item = String>,
) -> Result<(PathBuf, bool), String> {
    let mut offline = false;
    let mut source = None;
    for argument in std::iter::once(first).chain(arguments.by_ref()) {
        if argument == "--offline" {
            offline = true;
        } else if source.is_none() {
            source = Some(PathBuf::from(argument));
        } else {
            return Err(format!("unexpected argument `{argument}` after SOURCE"));
        }
    }
    source
        .map(|source| (source, offline))
        .ok_or_else(|| "--offline needs a SOURCE".to_owned())
}

fn run_cache(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let action = arguments.next();
    if let Some(extra) = arguments.next() {
        return Err(format!("unexpected cache argument `{extra}`"));
    }
    let config = AppConfig::load().map_err(|error| error.to_string())?;
    let cache = ProviderCache::platform_default(Some(
        config
            .cache_max_bytes()
            .map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    match action.as_deref() {
        None | Some("status") => show_cache_status(&cache),
        Some("clear") => clear_cache(&cache),
        Some(other) => Err(format!(
            "unknown cache action `{other}`; use `cache` or `cache clear`"
        )),
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
        "  Artwork: {} files, {}",
        status.artwork_entries,
        byte_count(status.artwork_bytes)
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

fn run_demo(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let mut scenario = None;
    let mut output = None;
    while let Some(argument) = arguments.next() {
        if argument == "--output" {
            let value = arguments
                .next()
                .ok_or_else(|| "--output needs a directory".to_owned())?;
            output = Some(PathBuf::from(value));
        } else if scenario.is_none() {
            scenario = Some(DemoScenario::parse(&argument).ok_or_else(|| {
                format!(
                    "unknown demo `{argument}`; use confident, ambiguous, matched-single, or standalone"
                )
            })?);
        } else {
            return Err(format!("unexpected argument `{argument}`"));
        }
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let styling = stdout.is_terminal() && env::var_os("NO_COLOR").is_none();
    let mut interaction = StdioInteraction::new(stdin.lock(), stdout.lock(), styling);
    demo::run(&mut interaction, scenario, output.as_deref())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn run_inspection(source: PathBuf, offline: bool) -> Result<(), String> {
    let inspection = SourceInspector::default()
        .inspect(&source)
        .map_err(|error| error.to_string())?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let styling = stdout.is_terminal() && env::var_os("NO_COLOR").is_none();
    let mut interaction = StdioInteraction::new(stdin.lock(), stdout.lock(), styling);
    if inspection.is_blocked() {
        inspection_ui::run(&mut interaction, &inspection)
            .map_err(|error| format!("terminal interaction failed: {error}"))?;
        Err("inspection found blocking problems; the source remains untouched".to_owned())
    } else {
        inspection_ui::run_before_matching(&mut interaction, &inspection)
            .map_err(|error| format!("terminal interaction failed: {error}"))?;
        let config = AppConfig::load().map_err(|error| error.to_string())?;
        let cache = ProviderCache::platform_default(Some(
            config
                .cache_max_bytes()
                .map_err(|error| error.to_string())?,
        ))
        .map_err(|error| error.to_string())?;
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

fn print_help() {
    println!("music-groomer 0.1.0 (milestone 3a in progress)");
    println!();
    println!("Inspect one album directory or loose audio file without changing it:");
    println!();
    println!("  music-groomer [--offline] SOURCE");
    println!();
    println!("Provider cache maintenance:");
    println!();
    println!("  music-groomer cache");
    println!("  music-groomer cache clear");
    println!();
    println!("Destination access and Apply are not implemented yet.");
    println!();
    println!("The Milestone 1 simulation remains available with:");
    println!();
    println!("  music-groomer demo [SCENARIO] [--output DIRECTORY]");
    println!();
    println!("Omit SCENARIO to choose within the guided session.");
    println!("The destination can also be changed inside the preview.");
    println!("Any --output directory must already exist.");
}
