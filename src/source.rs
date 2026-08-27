mod analysis;
mod artwork;
mod audio;
mod filesystem;
mod model;

pub use audio::{AudioReadError, LoftyAudioReader, PlannedTags};
pub use filesystem::{InspectionError, SourceInspector};
pub use model::{
    AncillaryFile, ArtworkCandidate, ArtworkFormat, AudioFormat, AudioProperties, AudioTags,
    InspectedAudio, InspectionNotice, NoticeKind, NoticeSeverity, SourceInspection,
};
