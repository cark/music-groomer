mod details;
mod render;
mod summary;

#[cfg(test)]
mod tests;

use std::io;

use crate::source::SourceInspection;
use crate::terminal::{Interaction, SemanticRole, UiLine};

pub fn run(interaction: &mut impl Interaction, inspection: &SourceInspection) -> io::Result<()> {
    run_menu(interaction, inspection, "[d] Done", false)
}

pub fn run_before_matching(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    run_menu(interaction, inspection, "[c] Continue to metadata", true)
}

fn run_menu(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
    completion: &str,
    continues_to_metadata: bool,
) -> io::Result<()> {
    summary::show(interaction, inspection, continues_to_metadata)?;
    loop {
        let answer = interaction
            .prompt(
                UiLine::new()
                    .with(SemanticRole::Prompt, "Choose: ")
                    .with(SemanticRole::MenuKey, "[r]")
                    .with(SemanticRole::Prompt, " Review files and tags  ")
                    .with(SemanticRole::MenuKey, &completion[..3])
                    .with(SemanticRole::Prompt, &completion[3..])
                    .with(SemanticRole::Prompt, ": "),
            )?
            .to_ascii_lowercase();
        match answer.as_str() {
            "r" | "review" => details::show(interaction, inspection)?,
            "" | "c" | "continue" | "d" | "done" | "q" | "quit" => return Ok(()),
            _ => interaction.error("Please choose Review files and tags or Done.")?,
        }
    }
}
