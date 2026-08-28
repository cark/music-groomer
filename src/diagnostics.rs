use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fs2::FileExt;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn initialize() -> Result<PathBuf, DiagnosticsError> {
    Diagnostics::open(&platform_path()?)?.install()
}

fn platform_path() -> Result<PathBuf, DiagnosticsError> {
    let directories = directories::ProjectDirs::from("", "", "music-groomer")
        .ok_or(DiagnosticsError::NoPlatformDirectory)?;
    let root = directories
        .state_dir()
        .unwrap_or_else(|| directories.cache_dir());
    Ok(root.join("diagnostics.log"))
}

#[derive(Debug)]
struct Diagnostics {
    path: PathBuf,
    file: File,
}

impl Diagnostics {
    fn open(path: &Path) -> Result<Self, DiagnosticsError> {
        let parent = path
            .parent()
            .ok_or_else(|| DiagnosticsError::InvalidPath(path.to_owned()))?;
        fs::create_dir_all(parent).map_err(|source| DiagnosticsError::CreateDirectory {
            path: parent.to_owned(),
            source,
        })?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|source| DiagnosticsError::Open {
                path: path.to_owned(),
                source,
            })?;
        file.try_lock_exclusive()
            .map_err(|source| DiagnosticsError::Lock {
                path: path.to_owned(),
                source,
            })?;
        file.set_len(0)
            .map_err(|source| DiagnosticsError::Truncate {
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            path: path.to_owned(),
            file,
        })
    }

    fn install(self) -> Result<PathBuf, DiagnosticsError> {
        let path = self.path;
        diagnostics_subscriber(self.file)
            .try_init()
            .map_err(|source| DiagnosticsError::Install(source.to_string()))?;
        tracing::info!(diagnostics = %path.display(), "diagnostics enabled");
        Ok(path)
    }
}

fn diagnostics_subscriber(file: File) -> impl tracing::Subscriber + Send + Sync + 'static {
    let filter = Targets::new()
        .with_target("music_groomer", tracing::Level::TRACE)
        .with_default(LevelFilter::OFF);
    let formatting = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_writer(Mutex::new(file))
        .with_filter(filter);
    tracing_subscriber::registry().with(formatting)
}

#[derive(Debug)]
pub enum DiagnosticsError {
    NoPlatformDirectory,
    InvalidPath(PathBuf),
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    Lock {
        path: PathBuf,
        source: std::io::Error,
    },
    Truncate {
        path: PathBuf,
        source: std::io::Error,
    },
    Install(String),
}

impl fmt::Display for DiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlatformDirectory => {
                formatter.write_str("this platform has no application diagnostics directory")
            }
            Self::InvalidPath(path) => write!(
                formatter,
                "diagnostics path has no parent directory: {}",
                path.display()
            ),
            Self::CreateDirectory { path, source } => write!(
                formatter,
                "cannot create diagnostics directory {}: {source}",
                path.display()
            ),
            Self::Open { path, source } => {
                write!(
                    formatter,
                    "cannot open diagnostics file {}: {source}",
                    path.display()
                )
            }
            Self::Lock { path, source } => write!(
                formatter,
                "another diagnostic run is using {}: {source}",
                path.display()
            ),
            Self::Truncate { path, source } => write!(
                formatter,
                "cannot replace diagnostics file {}: {source}",
                path.display()
            ),
            Self::Install(source) => write!(formatter, "cannot start diagnostics: {source}"),
        }
    }
}

impl std::error::Error for DiagnosticsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_diagnostics_truncates_the_previous_run() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("state/diagnostics.log");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old diagnostics").unwrap();

        let diagnostics = Diagnostics::open(&path).unwrap();

        assert_eq!(diagnostics.file.metadata().unwrap().len(), 0);
    }

    #[test]
    fn simultaneous_diagnostic_runs_are_refused() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("diagnostics.log");
        let first = Diagnostics::open(&path).unwrap();

        let error = Diagnostics::open(&path).unwrap_err();

        assert!(matches!(error, DiagnosticsError::Lock { .. }));
        drop(first);
        Diagnostics::open(&path).unwrap();
    }

    #[test]
    fn diagnostic_output_has_owned_timed_spans_without_dependency_noise() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("diagnostics.log");
        let diagnostics = Diagnostics::open(&path).unwrap();
        let subscriber = diagnostics_subscriber(diagnostics.file.try_clone().unwrap());

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "some_dependency", "must stay filtered");
            let span = tracing::info_span!(target: "music_groomer::test", "timed_test", path = "/music/track.flac");
            let _entered = span.enter();
            tracing::debug!(target: "music_groomer::test", kind = "flac", "classified");
        });
        drop(diagnostics);
        let output = fs::read_to_string(&path).unwrap();

        assert!(output.contains("timed_test"));
        assert!(output.contains("/music/track.flac"));
        assert!(output.contains("close time.busy="));
        assert!(!output.contains("must stay filtered"));
    }
}
