use std::io::{self, BufRead, Write};

use super::{Interaction, SemanticRole, UiLine};

pub struct StdioInteraction<R, W> {
    input: R,
    output: W,
    styling: bool,
}

impl<R, W> StdioInteraction<R, W> {
    pub fn new(input: R, output: W, styling: bool) -> Self {
        Self {
            input,
            output,
            styling,
        }
    }
}

impl<R: BufRead, W: Write> Interaction for StdioInteraction<R, W> {
    fn present(&mut self, line: UiLine) -> io::Result<()> {
        self.write_line(&line)?;
        self.output.write_all(b"\n")
    }

    fn prompt(&mut self, prompt: UiLine) -> io::Result<String> {
        self.write_line(&prompt)?;
        self.output.flush()?;
        let mut answer = String::new();
        self.input.read_line(&mut answer)?;
        Ok(answer.trim().to_owned())
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

        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "\x1b[1;36m[r]\x1b[0m Review \x1b[33mcarefully\x1b[0m\n"
        );
    }
}
