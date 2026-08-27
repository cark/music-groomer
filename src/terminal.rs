mod model;
mod render;

use std::io;

pub use model::{SemanticRole, UiLine, UiSpan};
pub use render::StdioInteraction;

pub trait Interaction {
    fn present(&mut self, line: UiLine) -> io::Result<()>;
    fn prompt(&mut self, prompt: UiLine) -> io::Result<String>;

    fn blank(&mut self) -> io::Result<()> {
        self.present(UiLine::blank())
    }

    fn prose(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::prose(text))
    }

    fn heading(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::heading(text))
    }

    fn success(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::success(text))
    }

    fn warning(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::warning(text))
    }

    fn error(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::error(text))
    }

    fn field(&mut self, name: impl Into<String>, value: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::field(name, value))
    }

    fn path_field(&mut self, name: impl Into<String>, path: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::path_field(name, path))
    }
}
