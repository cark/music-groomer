use std::collections::BTreeSet;

use super::{SemanticRole, UiLine};
mod catalogue;

use catalogue::MenuItem;
pub use catalogue::{Action, MenuId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResolvedItem {
    action: Action,
    key: char,
    label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionMenu {
    items: Vec<ResolvedItem>,
}

impl ActionMenu {
    pub fn for_id(id: MenuId) -> Self {
        let definitions = id.items();
        let mut items = Vec::with_capacity(definitions.len());
        let mut used = BTreeSet::new();
        let resolved = resolve_items(&definitions, 0, &mut used, &mut items);
        assert!(resolved, "menu {id:?} has no complete key assignment");
        Self { items }
    }

    pub fn prompt(&self, lead: impl Into<String>) -> UiLine {
        self.append_to(UiLine::new().with(SemanticRole::Prompt, lead))
    }

    pub fn append_to(&self, mut line: UiLine) -> UiLine {
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                line = line.with(SemanticRole::Prompt, "  ");
            }
            line = line
                .with(SemanticRole::MenuKey, format!("[{}]", item.key))
                .with(SemanticRole::Prompt, format!(" {}", item.label));
        }
        line.with(SemanticRole::Prompt, ": ")
    }

    pub fn action(&self, input: &str) -> Option<Action> {
        let input = input.trim().to_ascii_lowercase();
        self.items
            .iter()
            .find(|item| {
                input == item.key.to_string()
                    || item.action.aliases().iter().any(|alias| input == *alias)
            })
            .map(|item| item.action)
    }
}

fn resolve_items(
    definitions: &[MenuItem],
    index: usize,
    used: &mut BTreeSet<char>,
    resolved: &mut Vec<ResolvedItem>,
) -> bool {
    let Some(item) = definitions.get(index) else {
        return true;
    };
    for key in item.action.key_preferences() {
        if used.insert(*key) {
            resolved.push(ResolvedItem {
                action: item.action,
                key: *key,
                label: item.label,
            });
            if resolve_items(definitions, index + 1, used, resolved) {
                return true;
            }
            resolved.pop();
            used.remove(key);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn every_catalogued_menu_has_unique_keys_and_aliases() {
        for &id in MenuId::ALL {
            let menu = ActionMenu::for_id(id);
            let keys = menu
                .items
                .iter()
                .map(|item| item.key)
                .collect::<BTreeSet<_>>();
            assert_eq!(keys.len(), menu.items.len(), "duplicate key in {id:?}");

            let mut inputs = BTreeMap::new();
            for item in &menu.items {
                let key = item.key.to_string();
                assert!(inputs.insert(key.clone(), item.action).is_none());
                assert_eq!(menu.action(&key), Some(item.action));
                for alias in item.action.aliases() {
                    let previous = inputs.insert((*alias).to_owned(), item.action);
                    assert!(
                        previous.is_none() || previous == Some(item.action),
                        "ambiguous input {alias:?} in {id:?}"
                    );
                    assert_eq!(menu.action(alias), Some(item.action));
                }
            }
        }
    }

    #[test]
    fn earlier_actions_get_their_best_key_compatible_with_a_complete_solution() {
        let definitions = [
            MenuItem {
                action: Action::Artwork,
                label: "Artwork",
            },
            MenuItem {
                action: Action::Warnings,
                label: "Warnings",
            },
        ];
        let mut items = Vec::new();
        let mut used = BTreeSet::new();

        assert!(resolve_items(&definitions, 0, &mut used, &mut items));
        assert_eq!(items[0].key, 'a');
        assert_eq!(items[1].key, 'w');
    }

    #[test]
    fn metadata_and_exact_previews_use_the_same_artwork_key() {
        let metadata = ActionMenu::for_id(MenuId::MetadataPreviewRefresh);
        let exact = ActionMenu::for_id(MenuId::ExactPreview);

        assert_eq!(metadata.action("w"), Some(Action::Artwork));
        assert_eq!(exact.action("w"), Some(Action::Artwork));
        assert_eq!(metadata.action("a"), None);
    }

    #[test]
    fn prompt_rendering_and_parsing_share_the_resolved_assignment() {
        let menu = ActionMenu::for_id(MenuId::MetadataPreviewRefresh);
        let prompt = menu.prompt("Choose: ");

        assert_eq!(
            prompt.plain_text(),
            "Choose: [r] Review  [w] Artwork  [f] Refresh provider data and artwork  [d] Done: "
        );
        for item in &menu.items {
            assert_eq!(menu.action(&item.key.to_string()), Some(item.action));
        }
    }
}
