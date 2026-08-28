mod details;
mod render;
mod summary;

#[cfg(test)]
mod tests;

use std::io;

use crate::source::SourceInspection;
use crate::terminal::{Action, ActionMenu, Interaction, MenuId};

pub fn run(interaction: &mut impl Interaction, inspection: &SourceInspection) -> io::Result<()> {
    run_menu(interaction, inspection, false)
}

pub fn run_before_matching(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
) -> io::Result<()> {
    run_menu(interaction, inspection, true)
}

fn run_menu(
    interaction: &mut impl Interaction,
    inspection: &SourceInspection,
    continues_to_metadata: bool,
) -> io::Result<()> {
    summary::show(interaction, inspection, continues_to_metadata)?;
    let menu = ActionMenu::for_id(if continues_to_metadata {
        MenuId::InspectionContinue
    } else {
        MenuId::InspectionDone
    });
    loop {
        let answer = interaction.prompt(menu.prompt("Choose: "))?;
        match menu.action(&answer) {
            Some(Action::Review) => details::show(interaction, inspection)?,
            Some(Action::Continue) | Some(Action::Done) => return Ok(()),
            None if answer.is_empty() => return Ok(()),
            _ => interaction.error("Please choose one of the displayed actions.")?,
        }
    }
}
