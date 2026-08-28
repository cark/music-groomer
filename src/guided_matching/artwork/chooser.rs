use std::io;

use crate::artwork_viewer::ArtworkViewer;
use crate::provider::ProviderArtwork;
use crate::source::SourceInspection;
use crate::terminal::{Action, ActionMenu, Interaction, MenuId, SemanticRole, UiLine, byte_count};

use super::ArtworkSelection;

pub(in crate::guided_matching) fn choose_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    archive: Option<&ProviderArtwork>,
    current: ArtworkSelection,
    viewer: &mut V,
) -> io::Result<ArtworkSelection> {
    let choice_count = source.artwork.len() + usize::from(archive.is_some());
    if choice_count == 0 {
        interaction.warning("No album artwork is available.")?;
        return Ok(current);
    }
    let mut current = current;
    let menu = ActionMenu::for_id(MenuId::ArtworkChoice);
    loop {
        show_choices(interaction, source, archive, &current)?;
        let prompt = menu.append_to(
            UiLine::new()
                .with(SemanticRole::Prompt, "Choose: ")
                .with(SemanticRole::MenuKey, format!("[1-{choice_count}]"))
                .with(SemanticRole::Prompt, " Select  "),
        );
        let answer = interaction.prompt(prompt)?;
        match menu.action(&answer) {
            Some(Action::View) => {
                if let Some(choice) = prompt_view_choice(interaction, choice_count)? {
                    view_choice(interaction, source, archive, choice, viewer)?;
                }
            }
            Some(Action::Back) => return Ok(current),
            None if answer.is_empty() => {}
            _ => match parse_choice(&answer, choice_count) {
                Some(choice) => current = selection_for(source, archive, choice),
                None => interaction.error("Please select a listed number, View, or Back.")?,
            },
        }
    }
}

fn show_choices(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    archive: Option<&ProviderArtwork>,
    current: &ArtworkSelection,
) -> io::Result<()> {
    interaction.section_heading("Artwork choices")?;
    for (index, artwork) in source.artwork.iter().enumerate() {
        let selected = matches!(current, ArtworkSelection::Source(current) if *current == index);
        let bytes = source
            .ancillary
            .iter()
            .find(|file| file.relative_path == artwork.relative_path)
            .map(|file| byte_count(file.bytes))
            .unwrap_or_else(|| "size unknown".into());
        interaction.present(choice_line(
            index + 1,
            selected,
            "Source",
            Some(artwork.relative_path.display().to_string()),
            format!(
                "{}, {}×{}, {bytes}",
                artwork.format, artwork.dimensions.0, artwork.dimensions.1
            ),
        ))?;
    }
    if let Some(archive) = archive {
        interaction.present(choice_line(
            source.artwork.len() + 1,
            matches!(current, ArtworkSelection::CoverArtArchive(_)),
            "Cover Art Archive",
            Some("front".into()),
            format!(
                "{}, {}×{}, {}",
                archive.format,
                archive.dimensions.0,
                archive.dimensions.1,
                byte_count(archive.bytes.len() as u64)
            ),
        ))?;
    }
    interaction.blank()
}

fn choice_line(
    number: usize,
    selected: bool,
    provenance: &str,
    name: Option<String>,
    properties: String,
) -> UiLine {
    let mut line = UiLine::new()
        .with(SemanticRole::Prose, "  ")
        .with(
            if selected {
                SemanticRole::Selected
            } else {
                SemanticRole::Prose
            },
            if selected { "✓ " } else { "  " },
        )
        .with(SemanticRole::MenuKey, format!("{number}."))
        .with(SemanticRole::Prose, " ")
        .with(
            if selected {
                SemanticRole::Selected
            } else {
                SemanticRole::Alternative
            },
            provenance,
        );
    if let Some(name) = name {
        line = line
            .with(SemanticRole::Prose, " — ")
            .with(SemanticRole::Path, name);
    }
    line.with(SemanticRole::Prose, " — ")
        .with(SemanticRole::Value, properties)
}

fn prompt_view_choice(
    interaction: &mut impl Interaction,
    choice_count: usize,
) -> io::Result<Option<usize>> {
    let menu = ActionMenu::for_id(MenuId::ArtworkView);
    loop {
        let prompt = menu.append_to(
            UiLine::new()
                .with(SemanticRole::Prompt, "View which artwork? ")
                .with(SemanticRole::MenuKey, format!("[1-{choice_count}]"))
                .with(SemanticRole::Prompt, "  "),
        );
        let answer = interaction.prompt(prompt)?;
        if answer.is_empty() || menu.action(&answer) == Some(Action::Back) {
            return Ok(None);
        }
        if let Some(choice) = parse_choice(&answer, choice_count) {
            return Ok(Some(choice));
        }
        interaction.error("Please choose a listed artwork number or Back.")?;
    }
}

fn parse_choice(answer: &str, choice_count: usize) -> Option<usize> {
    answer
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|choice| (1..=choice_count).contains(choice))
}

fn selection_for(
    source: &SourceInspection,
    archive: Option<&ProviderArtwork>,
    choice: usize,
) -> ArtworkSelection {
    if choice <= source.artwork.len() {
        ArtworkSelection::Source(choice - 1)
    } else {
        ArtworkSelection::CoverArtArchive(
            archive
                .expect("the archive choice is counted only when artwork exists")
                .clone(),
        )
    }
}

fn view_choice<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    archive: Option<&ProviderArtwork>,
    choice: usize,
    viewer: &mut V,
) -> io::Result<()> {
    view_artwork(
        interaction,
        source,
        &selection_for(source, archive, choice),
        viewer,
    )
}

pub(in crate::guided_matching) fn view_artwork<V: ArtworkViewer>(
    interaction: &mut impl Interaction,
    source: &SourceInspection,
    artwork: &ArtworkSelection,
    viewer: &mut V,
) -> io::Result<()> {
    let result = match artwork {
        ArtworkSelection::Source(index) => source
            .artwork
            .get(*index)
            .ok_or_else(|| "the selected source cover is unavailable".to_owned())
            .and_then(|artwork| {
                let root = if source.kind == crate::domain::SourceKind::AlbumDirectory {
                    source.source.clone()
                } else {
                    source
                        .source
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .to_owned()
                };
                viewer
                    .view_path(&root.join(&artwork.relative_path))
                    .map_err(|error| error.to_string())
            }),
        ArtworkSelection::CoverArtArchive(artwork) => viewer
            .view_download(artwork)
            .map_err(|error| error.to_string()),
        ArtworkSelection::None => Err("no artwork is available to view".into()),
    };
    match result {
        Ok(()) => interaction.success("Opened the artwork choice in the system image viewer."),
        Err(error) => interaction.error(format!("Could not view artwork: {error}")),
    }
}
