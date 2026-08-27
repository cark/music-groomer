use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::domain::ArtistCredit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutTrack {
    pub title: String,
    pub disc: u16,
    pub track: u16,
    pub extension: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseLayout {
    pub album_artist: ArtistCredit,
    pub title: String,
    pub original_year: Option<u16>,
    pub disc_count: u16,
    pub tracks: Vec<LayoutTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneLayout {
    pub artist: ArtistCredit,
    pub title: String,
    pub extension: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedLayout {
    pub directory: PathBuf,
    pub tracks: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    EmptyComponent(&'static str),
    InvalidExtension(String),
    InvalidPosition { disc: u16, track: u16 },
    Collision(PathBuf),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyComponent(field) => {
                write!(formatter, "{field} is empty after filename sanitization")
            }
            Self::InvalidExtension(extension) => {
                write!(formatter, "invalid audio extension: {extension}")
            }
            Self::InvalidPosition { disc, track } => {
                write!(formatter, "invalid disc/track position {disc}-{track}")
            }
            Self::Collision(path) => {
                write!(formatter, "multiple tracks would use {}", path.display())
            }
        }
    }
}

impl std::error::Error for LayoutError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutPolicy;

impl LayoutPolicy {
    pub fn release(&self, release: &ReleaseLayout) -> Result<PlannedLayout, LayoutError> {
        let artist = component("album artist", &release.album_artist.display)?;
        let title = component("release title", &release.title)?;
        let release_directory = release
            .original_year
            .map_or_else(|| title.clone(), |year| format!("{year} - {title}"));
        let directory = PathBuf::from(artist).join(release_directory);
        let mut paths = Vec::with_capacity(release.tracks.len());
        let mut unique = BTreeSet::new();

        for source in &release.tracks {
            if source.disc == 0 || source.track == 0 {
                return Err(LayoutError::InvalidPosition {
                    disc: source.disc,
                    track: source.track,
                });
            }
            let title = component("track title", &source.title)?;
            let extension = extension(&source.extension)?;
            let number = if release.disc_count > 1 {
                format!("{:02}-{:02}", source.disc, source.track)
            } else {
                format!("{:02}", source.track)
            };
            let path = directory.join(format!("{number} - {title}.{extension}"));
            if !unique.insert(path.clone()) {
                return Err(LayoutError::Collision(path));
            }
            paths.push(path);
        }

        Ok(PlannedLayout {
            directory,
            tracks: paths,
        })
    }

    pub fn standalone(&self, standalone: &StandaloneLayout) -> Result<PlannedLayout, LayoutError> {
        let artist = component("artist", &standalone.artist.display)?;
        let title = component("track title", &standalone.title)?;
        let extension = extension(&standalone.extension)?;
        let directory = PathBuf::from(artist).join("Standalone Tracks").join(&title);
        let path = directory.join(format!("{title}.{extension}"));

        Ok(PlannedLayout {
            directory,
            tracks: vec![path],
        })
    }
}

fn component(field: &'static str, value: &str) -> Result<String, LayoutError> {
    let mut sanitized = String::with_capacity(value.len());
    let mut spacing = false;

    for character in value.chars() {
        let replacement = match character {
            '/' | '\\' => " - ",
            character if character.is_control() => " ",
            _ => {
                if spacing && !sanitized.is_empty() && !sanitized.ends_with(' ') {
                    sanitized.push(' ');
                }
                spacing = false;
                sanitized.push(character);
                continue;
            }
        };
        sanitized.push_str(replacement);
        spacing = true;
    }

    let collapsed = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let sanitized = collapsed.trim_matches([' ', '.']).to_owned();
    if sanitized.is_empty() {
        Err(LayoutError::EmptyComponent(field))
    } else {
        Ok(sanitized)
    }
}

fn extension(value: &str) -> Result<String, LayoutError> {
    let value = value.trim_start_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        Err(LayoutError::InvalidExtension(value))
    } else {
        Ok(value)
    }
}

pub fn relative_to<'a>(path: &'a Path, root: &Path) -> Option<&'a Path> {
    path.strip_prefix(root).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Artist;

    fn credit(display: &str, artists: &[&str]) -> ArtistCredit {
        ArtistCredit::credited(
            display,
            artists.iter().map(|name| Artist::named(*name)).collect(),
        )
    }

    #[test]
    fn lays_out_single_disc_album_without_disc_prefix() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Ten Years After"),
                title: "Evolution".into(),
                original_year: Some(1971),
                disc_count: 1,
                tracks: vec![LayoutTrack {
                    title: "I'd Love to Change the World".into(),
                    disc: 1,
                    track: 4,
                    extension: "FLAC".into(),
                }],
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.directory,
            PathBuf::from("Ten Years After/1971 - Evolution")
        );
        assert_eq!(
            layout.tracks,
            [PathBuf::from(
                "Ten Years After/1971 - Evolution/04 - I'd Love to Change the World.flac"
            )]
        );
    }

    #[test]
    fn lays_out_collaboration_using_full_credit() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: credit(
                    "Niels-Henning Ørsted Pedersen & Kenny Drew",
                    &["Niels-Henning Ørsted Pedersen", "Kenny Drew"],
                ),
                title: "Duo".into(),
                original_year: Some(1973),
                disc_count: 1,
                tracks: vec![LayoutTrack {
                    title: "In the Still of the Woods".into(),
                    disc: 1,
                    track: 1,
                    extension: "flac".into(),
                }],
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.directory,
            PathBuf::from("Niels-Henning Ørsted Pedersen & Kenny Drew/1973 - Duo")
        );
    }

    #[test]
    fn lays_out_compilation_under_various_artists() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Various Artists"),
                title: "A Sampler".into(),
                original_year: Some(1999),
                disc_count: 1,
                tracks: vec![LayoutTrack {
                    title: "Opening".into(),
                    disc: 1,
                    track: 1,
                    extension: "mp3".into(),
                }],
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.directory,
            PathBuf::from("Various Artists/1999 - A Sampler")
        );
    }

    #[test]
    fn lays_out_multi_disc_album_with_disc_prefix() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Artist"),
                title: "Long Album".into(),
                original_year: Some(2001),
                disc_count: 2,
                tracks: vec![
                    LayoutTrack {
                        title: "First".into(),
                        disc: 1,
                        track: 1,
                        extension: "m4a".into(),
                    },
                    LayoutTrack {
                        title: "Second".into(),
                        disc: 2,
                        track: 1,
                        extension: "m4a".into(),
                    },
                ],
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.tracks[1],
            PathBuf::from("Artist/2001 - Long Album/02-01 - Second.m4a")
        );
    }

    #[test]
    fn lays_out_matched_single_as_release() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Artist"),
                title: "A Great Single".into(),
                original_year: Some(1982),
                disc_count: 1,
                tracks: vec![LayoutTrack {
                    title: "A Great Song".into(),
                    disc: 1,
                    track: 1,
                    extension: "opus".into(),
                }],
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.tracks[0],
            PathBuf::from("Artist/1982 - A Great Single/01 - A Great Song.opus")
        );
    }

    #[test]
    fn lays_out_unmatched_standalone_without_invented_release_data() {
        let layout = LayoutPolicy
            .standalone(&StandaloneLayout {
                artist: ArtistCredit::single("Artist"),
                title: "Mystery Song".into(),
                extension: "ogg".into(),
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.tracks[0],
            PathBuf::from("Artist/Standalone Tracks/Mystery Song/Mystery Song.ogg")
        );
    }

    #[test]
    fn omits_an_unknown_year_instead_of_inventing_one() {
        let layout = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Artist"),
                title: "Undated Album".into(),
                original_year: None,
                disc_count: 1,
                tracks: vec![LayoutTrack {
                    title: "Track".into(),
                    disc: 1,
                    track: 1,
                    extension: "flac".into(),
                }],
            })
            .expect("missing year should be supported");

        assert_eq!(layout.directory, PathBuf::from("Artist/Undated Album"));
    }

    #[test]
    fn sanitizes_only_awkward_path_content_and_preserves_unicode() {
        let layout = LayoutPolicy
            .standalone(&StandaloneLayout {
                artist: ArtistCredit::single("Björk / Trio"),
                title: "One\nTwo\\Three".into(),
                extension: ".MP3".into(),
            })
            .expect("layout should be valid");

        assert_eq!(
            layout.tracks[0],
            PathBuf::from("Björk - Trio/Standalone Tracks/One Two - Three/One Two - Three.mp3")
        );
    }

    #[test]
    fn rejects_collisions_after_sanitization() {
        let error = LayoutPolicy
            .release(&ReleaseLayout {
                album_artist: ArtistCredit::single("Artist"),
                title: "Album".into(),
                original_year: Some(2000),
                disc_count: 1,
                tracks: vec![
                    LayoutTrack {
                        title: "Same/Title".into(),
                        disc: 1,
                        track: 1,
                        extension: "flac".into(),
                    },
                    LayoutTrack {
                        title: "Same\\Title".into(),
                        disc: 1,
                        track: 1,
                        extension: "flac".into(),
                    },
                ],
            })
            .expect_err("duplicate output must be rejected");

        assert!(matches!(error, LayoutError::Collision(_)));
    }
}
