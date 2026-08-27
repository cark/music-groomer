use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use music_groomer::demo::{self, DemoScenario};
use music_groomer::inspection_ui;
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
    if let Some(argument) = arguments.next() {
        return Err(format!("unexpected argument `{argument}` after SOURCE"));
    }
    run_inspection(PathBuf::from(command))
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

fn run_inspection(source: PathBuf) -> Result<(), String> {
    let inspection = SourceInspector::default()
        .inspect(&source)
        .map_err(|error| error.to_string())?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let styling = stdout.is_terminal() && env::var_os("NO_COLOR").is_none();
    let mut interaction = StdioInteraction::new(stdin.lock(), stdout.lock(), styling);
    inspection_ui::run(&mut interaction, &inspection)
        .map_err(|error| format!("terminal interaction failed: {error}"))?;
    if inspection.is_blocked() {
        Err("inspection found blocking problems; the source remains untouched".to_owned())
    } else {
        Ok(())
    }
}

fn print_help() {
    println!("music-groomer 0.1.0 (milestone 2)");
    println!();
    println!("Inspect one album directory or loose audio file without changing it:");
    println!();
    println!("  music-groomer SOURCE");
    println!();
    println!("Provider matching, destination access, and Apply are not implemented yet.");
    println!();
    println!("The Milestone 1 simulation remains available with:");
    println!();
    println!("  music-groomer demo [SCENARIO] [--output DIRECTORY]");
    println!();
    println!("Omit SCENARIO to choose within the guided session.");
    println!("The destination can also be changed inside the preview.");
    println!("Any --output directory must already exist.");
}
