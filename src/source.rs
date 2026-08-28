mod analysis;
mod artwork;
mod audio;
mod filesystem;
mod model;
mod mp4;
mod snapshot;

pub use audio::{
    AudioPreservationSnapshot, AudioReadError, LoftyAudioReader, PlannedTags, PreservedPicture,
    PreservedTagItem, PreservedTagValue,
};
pub use filesystem::{InspectionError, InspectionProgress, SourceInspector};
pub use model::{
    AncillaryFile, ArtworkCandidate, ArtworkFormat, AudioFormat, AudioProperties, AudioTags,
    InspectedAudio, InspectionNotice, NoticeKind, NoticeSeverity, SourceInspection,
    SourceObjectKind, SourceSnapshotEntry,
};
pub use snapshot::{SnapshotError, capture as capture_snapshot};
