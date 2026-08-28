use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Prepare one album or standalone track for a polished music library",
    long_about = "Inspect one selected album directory or standalone audio file, find plausible metadata and artwork, and preview the result without changing the source.",
    after_help = "Examples:\n  music-groomer /incoming/Album\n  music-groomer --offline /incoming/Album\n  music-groomer --diagnostics /incoming/Album\n  music-groomer --diagnostics=audio /incoming/Album\n  music-groomer --cache-dir /tmp/groomer-cache /incoming/Album\n  music-groomer cache\n  music-groomer cache clear\n  music-groomer recovery maintain"
)]
pub struct Cli {
    /// Write detailed timing diagnostics to music-groomer's standard state directory
    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "application",
        value_name = "SCOPE",
        requires = "source"
    )]
    pub diagnostics: Option<DiagnosticsArgument>,

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

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DiagnosticsArgument {
    /// Record only music-groomer's own diagnostic spans
    Application,

    /// Also record detailed Lofty and mp4parse audio-library events
    Audio,
}

impl DiagnosticsArgument {
    pub fn includes_audio_libraries(self) -> bool {
        matches!(self, Self::Audio)
    }
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Inspect or clear the provider cache
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
    },

    /// Inspect and maintain retained release recovery copies
    Recovery {
        #[command(subcommand)]
        action: RecoveryAction,
    },
}

#[derive(Clone, Copy, Debug, Subcommand)]
pub enum CacheAction {
    /// Show cache location, size, freshness, and damage information
    Status,

    /// Confirm and remove the selected music-groomer cache
    Clear,
}

#[derive(Clone, Copy, Debug, Subcommand)]
pub enum RecoveryAction {
    /// Evict eligible retained copies without prompting
    Maintain,
}
