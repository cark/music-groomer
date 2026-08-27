use std::io::{self, BufRead, Write};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::{Interaction, SemanticRole, UiLine};

pub struct StdioInteraction<R, W> {
    input: R,
    output: W,
    styling: bool,
    interactive: bool,
    status: Option<ProgressBar>,
}

impl<R, W> StdioInteraction<R, W> {
    pub fn new(input: R, output: W, styling: bool) -> Self {
        Self {
            input,
            output,
            styling,
            interactive: false,
            status: None,
        }
    }

    pub fn for_terminal(input: R, output: W, styling: bool) -> Self {
        Self {
            input,
            output,
            styling,
            interactive: true,
            status: None,
        }
    }
}

impl<R: BufRead, W: Write> Interaction for StdioInteraction<R, W> {
    fn present(&mut self, line: UiLine) -> io::Result<()> {
        self.clear_status();
        self.write_line(&line)?;
        self.output.write_all(b"\n")
    }

    fn prompt(&mut self, prompt: UiLine) -> io::Result<String> {
        self.clear_status();
        self.write_line(&prompt)?;
        self.output.flush()?;
        let mut answer = String::new();
        self.input.read_line(&mut answer)?;
        Ok(answer.trim().to_owned())
    }

    fn status(&mut self, line: UiLine) -> io::Result<()> {
        if !self.interactive {
            return self.present(line);
        }
        let styling = self.styling;
        let status = self.status.get_or_insert_with(|| new_status(styling));
        status.set_message(line.plain_text().trim().to_owned());
        Ok(())
    }
}

impl<R, W> StdioInteraction<R, W> {
    fn clear_status(&mut self) {
        if let Some(status) = self.status.take() {
            status.finish_and_clear();
        }
    }
}

impl<R, W: Write> StdioInteraction<R, W> {
    fn write_line(&mut self, line: &UiLine) -> io::Result<()> {
        for span in &line.spans {
            if self.styling {
                self.output.write_all(ansi(span.role).as_bytes())?;
            }
            self.output.write_all(span.text.as_bytes())?;
            if self.styling && !ansi(span.role).is_empty() {
                self.output.write_all(b"\x1b[0m")?;
            }
        }
        Ok(())
    }
}

impl<R, W> Drop for StdioInteraction<R, W> {
    fn drop(&mut self) {
        self.clear_status();
    }
}

fn new_status(styling: bool) -> ProgressBar {
    let status = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout());
    let template = if styling {
        "  {spinner:.cyan} {msg}"
    } else {
        "  {spinner} {msg}"
    };
    status.set_style(
        ProgressStyle::with_template(template)
            .expect("the terminal status template is valid")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    status.enable_steady_tick(Duration::from_millis(80));
    status
}

fn ansi(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Prose | SemanticRole::Prompt | SemanticRole::Alternative => "",
        SemanticRole::Heading | SemanticRole::Value | SemanticRole::Selected => "\x1b[1m",
        SemanticRole::FieldName => "\x1b[2;1m",
        SemanticRole::Path => "\x1b[36m",
        SemanticRole::Success => "\x1b[32m",
        SemanticRole::Warning => "\x1b[33m",
        SemanticRole::Error => "\x1b[31m",
        SemanticRole::MenuKey => "\x1b[1;36m",
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn plain_renderer_preserves_semantic_text() {
        let mut rendered = Vec::new();
        let mut terminal =
            StdioInteraction::new(Cursor::new(Vec::<u8>::new()), &mut rendered, false);
        terminal
            .present(UiLine::field("Album", "Evolution"))
            .unwrap();
        drop(terminal);

        assert_eq!(String::from_utf8(rendered).unwrap(), "  Album: Evolution\n");
    }

    #[test]
    fn colored_renderer_uses_the_central_palette() {
        let mut rendered = Vec::new();
        let mut terminal =
            StdioInteraction::new(Cursor::new(Vec::<u8>::new()), &mut rendered, true);
        terminal
            .present(
                UiLine::new()
                    .with(SemanticRole::MenuKey, "[r]")
                    .with(SemanticRole::Prose, " Review ")
                    .with(SemanticRole::Warning, "carefully"),
            )
            .unwrap();
        drop(terminal);

        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "\x1b[1;36m[r]\x1b[0m Review \x1b[33mcarefully\x1b[0m\n"
        );
    }

    #[test]
    fn non_interactive_status_is_stable_output() {
        let mut rendered = Vec::new();
        let mut terminal =
            StdioInteraction::new(Cursor::new(Vec::<u8>::new()), &mut rendered, false);

        terminal
            .status(UiLine::prose("  MusicBrainz search..."))
            .unwrap();
        drop(terminal);

        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "  MusicBrainz search...\n"
        );
    }

    #[test]
    fn interactive_status_is_transient_and_cleared_by_persistent_output() {
        let mut rendered = Vec::new();
        let mut terminal =
            StdioInteraction::for_terminal(Cursor::new(Vec::<u8>::new()), &mut rendered, false);

        terminal
            .status(UiLine::prose("MusicBrainz search..."))
            .unwrap();
        assert!(terminal.status.is_some());
        terminal.success("MusicBrainz lookup completed.").unwrap();
        assert!(terminal.status.is_none());
        drop(terminal);

        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "MusicBrainz lookup completed.\n"
        );
    }

    #[test]
    fn section_heading_has_a_consistent_visual_break() {
        let mut rendered = Vec::new();
        let mut terminal =
            StdioInteraction::new(Cursor::new(Vec::<u8>::new()), &mut rendered, false);

        terminal.section_heading("Metadata preview").unwrap();
        drop(terminal);

        assert_eq!(String::from_utf8(rendered).unwrap(), "\nMetadata preview\n");
    }
}
