#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Apply,
    Artwork,
    Back,
    Cancel,
    Continue,
    Destination,
    Done,
    ExistingTags,
    Identification,
    Metadata,
    More,
    Refresh,
    Review,
    SaveDefault,
    SourceDetails,
    TrackDetails,
    UseOnce,
    View,
    Warnings,
}

impl Action {
    pub(super) fn key_preferences(self) -> &'static [char] {
        match self {
            Self::Apply => &['a', 'p'],
            Self::Artwork => &['w', 'a'],
            Self::Back => &['b'],
            Self::Cancel => &['c', 'q'],
            Self::Continue => &['c'],
            Self::Destination => &['d', 'o'],
            Self::Done => &['d', 'n'],
            Self::ExistingTags => &['e'],
            Self::Identification => &['i'],
            Self::Metadata => &['m'],
            Self::More => &['m'],
            Self::Refresh => &['f', 'r'],
            Self::Review => &['r'],
            Self::SaveDefault => &['s'],
            Self::SourceDetails => &['s'],
            Self::TrackDetails => &['t'],
            Self::UseOnce => &['o', 'u'],
            Self::View => &['v'],
            Self::Warnings => &['w'],
        }
    }

    pub(super) fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Apply => &["apply"],
            Self::Artwork => &["artwork"],
            Self::Back => &["back"],
            Self::Cancel => &["cancel", "quit", "q"],
            Self::Continue => &["continue", "quit", "q"],
            Self::Destination => &["destination"],
            Self::Done => &["done", "quit", "q"],
            Self::ExistingTags => &["existing"],
            Self::Identification => &["identification"],
            Self::Metadata => &["metadata"],
            Self::More => &["more"],
            Self::Refresh => &["refresh"],
            Self::Review => &["review"],
            Self::SaveDefault => &["save"],
            Self::SourceDetails => &["source"],
            Self::TrackDetails => &["tracks", "details"],
            Self::UseOnce => &["once"],
            Self::View => &["view"],
            Self::Warnings => &["warnings"],
        }
    }
}

macro_rules! menu_catalogue {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum MenuId {
            $($variant),+
        }

        #[cfg(test)]
        impl MenuId {
            pub(super) const ALL: &[Self] = &[$(Self::$variant),+];
        }
    };
}

menu_catalogue!(
    InspectionDone,
    InspectionContinue,
    CandidateChoice,
    CandidateChoiceExisting,
    CandidateChoiceMore,
    CandidateChoiceMoreExisting,
    SingleCandidate,
    SingleCandidateExisting,
    MetadataRevision,
    MetadataRevisionExisting,
    MetadataPreview,
    MetadataPreviewRefresh,
    MetadataReview,
    MetadataReviewIdentification,
    ExactPreview,
    DestinationChoice,
    ArtworkChoice,
    ArtworkView,
);

impl MenuId {
    pub(super) fn items(self) -> Vec<MenuItem> {
        match self {
            Self::InspectionDone => vec![
                MenuItem::new(Action::Review, "Review files and tags"),
                MenuItem::new(Action::Done, "Done"),
            ],
            Self::InspectionContinue => vec![
                MenuItem::new(Action::Review, "Review files and tags"),
                MenuItem::new(Action::Continue, "Continue to metadata"),
            ],
            Self::CandidateChoice
            | Self::CandidateChoiceExisting
            | Self::CandidateChoiceMore
            | Self::CandidateChoiceMoreExisting => candidate_items(self),
            Self::SingleCandidate | Self::SingleCandidateExisting => {
                let mut items = vec![MenuItem::new(Action::TrackDetails, "Track-list details")];
                if self == Self::SingleCandidateExisting {
                    items.push(MenuItem::new(Action::ExistingTags, "Existing tags"));
                }
                items
            }
            Self::MetadataRevision | Self::MetadataRevisionExisting => {
                let mut items = vec![MenuItem::new(Action::TrackDetails, "Track-list details")];
                if self == Self::MetadataRevisionExisting {
                    items.push(MenuItem::new(Action::ExistingTags, "Existing tags"));
                }
                items.push(MenuItem::new(Action::Back, "Back"));
                items
            }
            Self::MetadataPreview | Self::MetadataPreviewRefresh => {
                let mut items = vec![
                    MenuItem::new(Action::Review, "Review"),
                    MenuItem::new(Action::Artwork, "Artwork"),
                ];
                if self == Self::MetadataPreviewRefresh {
                    items.push(MenuItem::new(
                        Action::Refresh,
                        "Refresh provider data and artwork",
                    ));
                }
                items.push(MenuItem::new(Action::Done, "Done"));
                items
            }
            Self::MetadataReview | Self::MetadataReviewIdentification => {
                let mut items = vec![
                    MenuItem::new(Action::SourceDetails, "Source files and tags"),
                    MenuItem::new(Action::Metadata, "Metadata"),
                ];
                if self == Self::MetadataReviewIdentification {
                    items.push(MenuItem::new(Action::Identification, "Identification"));
                }
                items.extend([
                    MenuItem::new(Action::Warnings, "Warnings"),
                    MenuItem::new(Action::Back, "Back"),
                ]);
                items
            }
            Self::ExactPreview => vec![
                MenuItem::new(Action::Apply, "Apply"),
                MenuItem::new(Action::Review, "Review changes"),
                MenuItem::new(Action::Artwork, "Artwork"),
                MenuItem::new(Action::Destination, "Destination"),
                MenuItem::new(Action::Cancel, "Cancel"),
            ],
            Self::DestinationChoice => vec![
                MenuItem::new(Action::UseOnce, "Use once"),
                MenuItem::new(Action::SaveDefault, "Use and save as default"),
                MenuItem::new(Action::Back, "Go back"),
            ],
            Self::ArtworkChoice => vec![
                MenuItem::new(Action::View, "View a choice"),
                MenuItem::new(Action::Back, "Back"),
            ],
            Self::ArtworkView => vec![MenuItem::new(Action::Back, "Back")],
        }
    }
}

fn candidate_items(menu: MenuId) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if matches!(
        menu,
        MenuId::CandidateChoiceMore | MenuId::CandidateChoiceMoreExisting
    ) {
        items.push(MenuItem::new(Action::More, "Show more"));
    }
    items.push(MenuItem::new(Action::TrackDetails, "Track-list details"));
    if matches!(
        menu,
        MenuId::CandidateChoiceExisting | MenuId::CandidateChoiceMoreExisting
    ) {
        items.push(MenuItem::new(
            Action::ExistingTags,
            "Use existing tags (unverified)",
        ));
    }
    items.push(MenuItem::new(Action::Cancel, "Cancel"));
    items
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MenuItem {
    pub(super) action: Action,
    pub(super) label: &'static str,
}

impl MenuItem {
    const fn new(action: Action, label: &'static str) -> Self {
        Self { action, label }
    }
}
