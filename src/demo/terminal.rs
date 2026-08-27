use std::io::{self, BufRead, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyle {
    Heading,
    Success,
    Warning,
    Error,
    Value,
    Path,
    Label,
}

impl TextStyle {
    fn ansi(self) -> &'static str {
        match self {
            Self::Heading | Self::Label => "\x1b[1m",
            Self::Success => "\x1b[32m",
            Self::Warning => "\x1b[33m",
            Self::Error => "\x1b[31m",
            Self::Value | Self::Path => "\x1b[36m",
        }
    }
}

pub trait Interaction {
    fn show(&mut self, text: &str) -> io::Result<()>;
    fn ask(&mut self, prompt: &str) -> io::Result<String>;

    fn styled(&self, _style: TextStyle, text: &str) -> String {
        text.to_owned()
    }
}

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
    fn show(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.output, "{text}")
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        write!(self.output, "{prompt}")?;
        self.output.flush()?;
        let mut answer = String::new();
        self.input.read_line(&mut answer)?;
        Ok(answer.trim().to_owned())
    }

    fn styled(&self, style: TextStyle, text: &str) -> String {
        if self.styling {
            format!("{}{text}\x1b[0m", style.ansi())
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn styling_is_optional_and_never_changes_the_text_itself() {
        let plain = StdioInteraction::new(Cursor::new(Vec::<u8>::new()), Vec::new(), false);
        let styled = StdioInteraction::new(Cursor::new(Vec::<u8>::new()), Vec::new(), true);

        assert_eq!(plain.styled(TextStyle::Warning, "Warning"), "Warning");
        assert_eq!(
            styled.styled(TextStyle::Warning, "Warning"),
            "\x1b[33mWarning\x1b[0m"
        );
    }
}
