#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticRole {
    Prose,
    Heading,
    FieldName,
    Value,
    Path,
    Success,
    Warning,
    Error,
    MenuKey,
    Prompt,
    Selected,
    Alternative,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSpan {
    pub role: SemanticRole,
    pub text: String,
}

impl UiSpan {
    pub fn new(role: SemanticRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiLine {
    pub spans: Vec<UiSpan>,
}

impl UiLine {
    pub fn blank() -> Self {
        Self::default()
    }

    pub fn prose(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Prose, text)
    }

    pub fn heading(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Heading, text)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Success, text)
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Warning, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Error, text)
    }

    pub fn prompt(text: impl Into<String>) -> Self {
        Self::single(SemanticRole::Prompt, text)
    }

    pub fn confirmation_prompt(text: impl AsRef<str>) -> Self {
        let text = text.as_ref();
        let mut line = Self::new();
        let mut rest = text;
        while let Some(start) = rest.find('[') {
            let (before, key_and_after) = rest.split_at(start);
            if !before.is_empty() {
                line = line.with(SemanticRole::Prompt, before);
            }
            let Some(end) = key_and_after.find(']') else {
                rest = key_and_after;
                break;
            };
            let (key, after) = key_and_after.split_at(end + 1);
            line = line.with(SemanticRole::MenuKey, key);
            rest = after;
        }
        if !rest.is_empty() {
            line = line.with(SemanticRole::Prompt, rest);
        }
        line
    }

    pub fn field(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new()
            .with(SemanticRole::Prose, "  ")
            .with(SemanticRole::FieldName, name)
            .with(SemanticRole::Prose, ": ")
            .with(SemanticRole::Value, value)
    }

    pub fn path_field(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new()
            .with(SemanticRole::Prose, "  ")
            .with(SemanticRole::FieldName, name)
            .with(SemanticRole::Prose, ": ")
            .with(SemanticRole::Path, path)
    }

    pub fn menu_item(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new()
            .with(SemanticRole::Prose, "  ")
            .with(SemanticRole::MenuKey, key)
            .with(SemanticRole::Prose, " ")
            .with(SemanticRole::Alternative, label)
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, role: SemanticRole, text: impl Into<String>) -> Self {
        self.spans.push(UiSpan::new(role, text));
        self
    }

    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn single(role: SemanticRole, text: impl Into<String>) -> Self {
        Self {
            spans: vec![UiSpan::new(role, text)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_prompt_marks_choices_without_changing_readable_text() {
        let prompt = UiLine::confirmation_prompt("Continue? [y/N]: ");

        assert_eq!(prompt.plain_text(), "Continue? [y/N]: ");
        assert_eq!(
            prompt
                .spans
                .iter()
                .filter(|span| span.role == SemanticRole::MenuKey)
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            ["[y/N]"]
        );
    }
}
