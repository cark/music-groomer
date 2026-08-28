mod action;
mod model;
mod render;

use std::io;

pub use action::{Action, ActionMenu, MenuId};
pub use model::{SemanticRole, UiLine, UiSpan};
pub use render::StdioInteraction;

pub fn byte_count(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub trait Interaction {
    fn present(&mut self, line: UiLine) -> io::Result<()>;
    fn prompt(&mut self, prompt: UiLine) -> io::Result<String>;

    fn status(&mut self, line: UiLine) -> io::Result<()> {
        self.present(line)
    }

    fn blank(&mut self) -> io::Result<()> {
        self.present(UiLine::blank())
    }

    fn prose(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::prose(text))
    }

    fn heading(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.present(UiLine::heading(text))
    }

    fn section_heading(&mut self, text: impl Into<String>) -> io::Result<()> {
        self.blank()?;
        self.heading(text)
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

#[cfg(test)]
mod tests {
    use super::byte_count;

    #[test]
    fn byte_counts_use_the_largest_readable_binary_unit() {
        assert_eq!(byte_count(512), "512 B");
        assert_eq!(byte_count(2 * 1024), "2.0 KiB");
        assert_eq!(byte_count(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(byte_count(10 * 1024 * 1024 * 1024), "10.0 GiB");
    }
}
