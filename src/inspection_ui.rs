mod details;
mod render;
mod summary;

#[cfg(test)]
mod tests;

use std::io;

use crate::source::SourceInspection;
use crate::terminal::{Interaction, TextStyle};

pub fn run(interaction: &mut impl Interaction, inspection: &SourceInspection) -> io::Result<()> {
    summary::show(interaction, inspection)?;
    loop {
        let review = interaction.styled(TextStyle::Label, "[r] Review files and tags");
        let done = interaction.styled(TextStyle::Label, "[d] Done");
        let answer = interaction
            .ask(&format!("Choose: {review}  {done}: "))?
            .to_ascii_lowercase();
        match answer.as_str() {
            "r" | "review" => details::show(interaction, inspection)?,
            "" | "d" | "done" | "q" | "quit" => return Ok(()),
            _ => render::show_styled(
                interaction,
                TextStyle::Error,
                "Please choose Review files and tags or Done.",
            )?,
        }
    }
}
