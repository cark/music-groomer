use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use music_groomer::demo::{self, DemoScenario, StdioInteraction};

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
    if command != "demo" {
        return Err(
            "real source processing is not implemented yet; use `music-groomer demo`".to_owned(),
        );
    }

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

fn print_help() {
    println!("music-groomer 0.1.0 (milestone 1)");
    println!();
    println!("The real file workflow is not implemented yet.");
    println!("Run the safe guided simulation with:");
    println!();
    println!("  music-groomer demo [SCENARIO] [--output DIRECTORY]");
    println!();
    println!("Omit SCENARIO to choose within the guided session.");
    println!("The destination can also be changed inside the preview.");
    println!("Any --output directory must already exist.");
}
