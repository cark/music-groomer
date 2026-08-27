use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use crate::provider::ProviderArtwork;

pub trait ArtworkViewer {
    fn view_path(&mut self, path: &Path) -> Result<(), ViewerError>;
    fn view_download(&mut self, artwork: &ProviderArtwork) -> Result<(), ViewerError>;
}

#[derive(Default)]
pub struct SystemArtworkViewer {
    temporary_directories: Vec<tempfile::TempDir>,
    children: Vec<Child>,
}

impl SystemArtworkViewer {
    pub fn new() -> Self {
        Self::default()
    }

    fn launch(&mut self, path: &Path) -> Result<(), ViewerError> {
        self.reap_finished();
        let child = platform_command(path)
            .spawn()
            .map_err(|error| ViewerError::Launch(path.to_owned(), error))?;
        self.children.push(child);
        Ok(())
    }

    fn reap_finished(&mut self) {
        self.children
            .retain_mut(|child| child.try_wait().ok().flatten().is_none());
    }
}

impl ArtworkViewer for SystemArtworkViewer {
    fn view_path(&mut self, path: &Path) -> Result<(), ViewerError> {
        self.launch(path)
    }

    fn view_download(&mut self, artwork: &ProviderArtwork) -> Result<(), ViewerError> {
        let directory = tempfile::Builder::new()
            .prefix("music-groomer-artwork-")
            .tempdir()
            .map_err(ViewerError::Temporary)?;
        let path = directory
            .path()
            .join(format!("cover.{}", artwork.format.canonical_extension()));
        fs::write(&path, &artwork.bytes)
            .map_err(|error| ViewerError::Write(path.clone(), error))?;
        self.launch(&path)?;
        self.temporary_directories.push(directory);
        Ok(())
    }
}

#[derive(Debug)]
pub enum ViewerError {
    Temporary(std::io::Error),
    Write(PathBuf, std::io::Error),
    Launch(PathBuf, std::io::Error),
}

impl fmt::Display for ViewerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Temporary(error) => {
                write!(formatter, "cannot prepare temporary artwork: {error}")
            }
            Self::Write(path, error) => {
                write!(
                    formatter,
                    "cannot prepare {} for viewing: {error}",
                    path.display()
                )
            }
            Self::Launch(path, error) => {
                write!(
                    formatter,
                    "cannot open {} in an image viewer: {error}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ViewerError {}

#[cfg(target_os = "windows")]
fn platform_command(path: &Path) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", ""]).arg(path);
    command
}

#[cfg(target_os = "macos")]
fn platform_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg(path);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_command(path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}
