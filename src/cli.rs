use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Prepare one album or standalone track for a polished music library",
    long_about = "Inspect one selected album directory or standalone audio file, find plausible metadata and artwork, and preview the result without changing the source.",
    after_help = "Examples:\n  music-groomer /incoming/Album\n  music-groomer --offline /incoming/Album\n  music-groomer --diagnostics /incoming/Album\n  music-groomer --cache-dir /tmp/groomer-cache /incoming/Album\n  music-groomer cache\n  music-groomer cache clear"
)]
pub struct Cli {
    /// Write detailed timing diagnostics to music-groomer's standard state directory
    #[arg(long, requires = "source")]
    pub diagnostics: bool,

    /// Use this exact provider cache directory for this invocation
    #[arg(long, global = true, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    pub cache_dir: Option<PathBuf>,

    /// Do not contact metadata or artwork providers
    #[arg(long, requires = "source")]
    pub offline: bool,

    /// Use this existing destination root for this invocation
    #[arg(short, long, requires = "source", value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
    pub output: Option<PathBuf>,

    /// Album directory or standalone audio file to groom
    #[arg(value_name = "SOURCE", value_hint = ValueHint::AnyPath)]
    pub source: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Inspect or clear the provider cache
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
    },

    /// Run an internal simulated workflow
    #[command(hide = true)]
    Demo {
        /// Simulated source and matching situation
        #[arg(value_enum)]
        scenario: Option<DemoScenarioArgument>,

        /// Existing directory used as the simulated destination
        #[arg(long, value_name = "DIRECTORY", value_hint = ValueHint::DirPath)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, Subcommand)]
pub enum CacheAction {
    /// Show cache location, size, freshness, and damage information
    Status,

    /// Confirm and remove the selected music-groomer cache
    Clear,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DemoScenarioArgument {
    Confident,
    Ambiguous,
    MatchedSingle,
    Standalone,
}

impl DemoScenarioArgument {
    pub fn name(self) -> &'static str {
        match self {
            Self::Confident => "confident",
            Self::Ambiguous => "ambiguous",
            Self::MatchedSingle => "matched-single",
            Self::Standalone => "standalone",
        }
    }
}
