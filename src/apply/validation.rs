use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::plan::{ArtworkOrigin, GroomingPlan};
use crate::planning::source_root;
use crate::source::{
    AudioTags, InspectedAudio, LoftyAudioReader, PlannedTags, SourceInspection, SourceInspector,
    SourceObjectKind, capture_snapshot,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationReport {
    pub tracks: usize,
    pub artwork_files: usize,
}

#[derive(Debug)]
pub struct ValidationError {
    pub path: Option<PathBuf>,
    pub invariant: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(formatter, "{}: {}", path.display(), self.invariant)
        } else {
            formatter.write_str(&self.invariant)
        }
    }
}

impl std::error::Error for ValidationError {}

pub fn validate(
    source: &SourceInspection,
    plan: &GroomingPlan,
    stage: &Path,
) -> Result<ValidationReport, ValidationError> {
    validate_file_set(plan, stage)?;
    for directory in &plan.ancillary_directories {
        if !stage.join(directory).is_dir() {
            return Err(mismatch(
                &stage.join(directory),
                "planned ancillary directory is missing",
            ));
        }
    }
    let staged = SourceInspector::default()
        .inspect(stage)
        .map_err(|error| mismatch(stage, format!("cannot re-inspect staged result: {error}")))?;
    if staged.is_blocked() {
        let notice = staged
            .notices
            .iter()
            .find(|notice| notice.severity == crate::source::NoticeSeverity::Blocker)
            .expect("blocked inspection has a blocker");
        return Err(ValidationError {
            path: notice.path.as_ref().map(|path| stage.join(path)),
            invariant: notice.message.clone(),
        });
    }
    let staged_audio = staged
        .audio
        .iter()
        .map(|audio| (audio.relative_path.clone(), audio))
        .collect::<BTreeMap<_, _>>();
    let source_root = source_root(source);
    let reader = LoftyAudioReader;
    for track in &plan.tracks {
        let relative = relative_track(plan, track)?;
        let path = stage.join(relative);
        let actual = staged_audio
            .get(relative)
            .ok_or_else(|| mismatch(&path, "planned audio file is missing"))?;
        let original = source
            .audio
            .iter()
            .find(|audio| audio.relative_path == track.source_relative)
            .ok_or_else(|| {
                mismatch(&path, "planned source audio is no longer in the inspection")
            })?;
        validate_audio_tags(&path, original, actual, track.planned_tags.as_ref())?;
        if actual.format != original.format || actual.properties != original.properties {
            return Err(mismatch(
                &path,
                "audio format or codec properties changed during grooming",
            ));
        }
        let before = reader
            .preservation_snapshot(&source_root.join(&track.source_relative))
            .map_err(|error| mismatch(&path, format!("cannot re-read source tags: {error}")))?;
        let after = reader
            .preservation_snapshot(&path)
            .map_err(|error| mismatch(&path, format!("cannot re-read staged tags: {error}")))?;
        if before != after {
            return Err(mismatch(
                &path,
                "unrelated tags or embedded artwork were not preserved exactly",
            ));
        }
    }
    validate_ancillary(source, plan, stage, &source_root)?;
    validate_artwork(source, plan, stage, &source_root)?;
    Ok(ValidationReport {
        tracks: plan.tracks.len(),
        artwork_files: usize::from(plan.artwork.output_name.is_some()),
    })
}

fn validate_file_set(plan: &GroomingPlan, stage: &Path) -> Result<(), ValidationError> {
    let expected = plan
        .tracks
        .iter()
        .map(|track| relative_track(plan, track).map(Path::to_owned))
        .chain(
            plan.ancillary
                .iter()
                .map(|file| Ok(file.destination_relative.clone())),
        )
        .chain(
            plan.artwork
                .output_name
                .iter()
                .map(|name| Ok(PathBuf::from(name))),
        )
        .collect::<Result<BTreeSet<_>, ValidationError>>()?;
    let snapshot = capture_snapshot(stage, crate::domain::SourceKind::AlbumDirectory)
        .map_err(|error| mismatch(&error.path, error.to_string()))?;
    let mut actual = BTreeSet::new();
    for entry in snapshot {
        match entry.kind {
            SourceObjectKind::File => {
                actual.insert(entry.relative_path);
            }
            SourceObjectKind::Directory => {}
            SourceObjectKind::Symlink | SourceObjectKind::Special => {
                return Err(mismatch(
                    &stage.join(entry.relative_path),
                    "staged result contains a symbolic link or special object",
                ));
            }
        }
    }
    if actual != expected {
        return Err(mismatch(
            stage,
            format!(
                "staged file set differs from the preview: expected {expected:?}, found {actual:?}"
            ),
        ));
    }
    Ok(())
}

fn validate_audio_tags(
    path: &Path,
    original: &InspectedAudio,
    actual: &InspectedAudio,
    planned: Option<&PlannedTags>,
) -> Result<(), ValidationError> {
    let expected = planned.map_or_else(
        || original.tags.clone(),
        |planned| expected_tags(&original.tags, planned),
    );
    if actual.tags != expected {
        return Err(mismatch(
            path,
            format!(
                "active tags differ from the preview: expected {:?}, found {:?}",
                expected, actual.tags
            ),
        ));
    }
    Ok(())
}

fn expected_tags(source: &AudioTags, planned: &PlannedTags) -> AudioTags {
    AudioTags {
        title: Some(planned.title.clone()),
        artist: Some(planned.artist.clone()),
        artists: planned.artists.clone(),
        album: Some(planned.album.clone()),
        album_artist: Some(planned.album_artist.clone()),
        album_artists: planned.album_artists.clone(),
        artist_ids: planned
            .artist_ids
            .clone()
            .unwrap_or_else(|| source.artist_ids.clone()),
        album_artist_ids: planned
            .album_artist_ids
            .clone()
            .unwrap_or_else(|| source.album_artist_ids.clone()),
        compilation: Some(planned.compilation),
        date: planned
            .original_year
            .map(|year| year.to_string())
            .or_else(|| source.date.clone()),
        track: Some(planned.track),
        track_total: Some(planned.track_total),
        disc: Some(planned.disc),
        disc_total: Some(planned.disc_total),
        recording_id: planned
            .recording_id
            .clone()
            .or_else(|| source.recording_id.clone()),
        release_group_id: planned
            .release_group_id
            .clone()
            .or_else(|| source.release_group_id.clone()),
        embedded_pictures: source.embedded_pictures,
    }
}

fn validate_ancillary(
    _source: &SourceInspection,
    plan: &GroomingPlan,
    stage: &Path,
    source_root: &Path,
) -> Result<(), ValidationError> {
    #[cfg(unix)]
    for directory in &plan.ancillary_directories {
        use std::os::unix::fs::PermissionsExt;
        let before = source_root.join(directory);
        let after = stage.join(directory);
        let before_mode = fs::metadata(&before)
            .map_err(|error| mismatch(&before, error.to_string()))?
            .permissions()
            .mode();
        let after_mode = fs::metadata(&after)
            .map_err(|error| mismatch(&after, error.to_string()))?
            .permissions()
            .mode();
        if before_mode != after_mode {
            return Err(mismatch(
                &after,
                "ancillary directory permission bits changed",
            ));
        }
    }
    for file in &plan.ancillary {
        let before = source_root.join(&file.source_relative);
        let after = stage.join(&file.destination_relative);
        if digest(&before)? != digest(&after)? {
            return Err(mismatch(&after, "ancillary file bytes changed"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let before_mode = fs::metadata(&before)
                .map_err(|error| mismatch(&before, error.to_string()))?
                .permissions()
                .mode();
            let after_mode = fs::metadata(&after)
                .map_err(|error| mismatch(&after, error.to_string()))?
                .permissions()
                .mode();
            if before_mode != after_mode {
                return Err(mismatch(&after, "ancillary Unix permission bits changed"));
            }
        }
    }
    Ok(())
}

fn validate_artwork(
    source: &SourceInspection,
    plan: &GroomingPlan,
    stage: &Path,
    source_root: &Path,
) -> Result<(), ValidationError> {
    let Some(output_name) = &plan.artwork.output_name else {
        return Ok(());
    };
    let path = stage.join(output_name);
    let expected = match &plan.artwork.origin {
        ArtworkOrigin::SourceSidecar { source_name } => digest(&source_root.join(source_name))?,
        ArtworkOrigin::CoverArtArchive { .. } => {
            let bytes = plan
                .archive_artwork_bytes
                .as_ref()
                .ok_or_else(|| mismatch(&path, "selected archive artwork has no planned bytes"))?;
            Sha256::digest(bytes).into()
        }
        ArtworkOrigin::None => {
            let _ = source;
            return Err(mismatch(
                &path,
                "artwork output was planned without an origin",
            ));
        }
    };
    if digest(&path)? != expected {
        return Err(mismatch(&path, "canonical artwork bytes changed"));
    }
    let bytes = fs::read(&path).map_err(|error| mismatch(&path, error.to_string()))?;
    let format = image::guess_format(&bytes)
        .map_err(|error| mismatch(&path, format!("cannot recognize staged artwork: {error}")))?;
    let image = image::load_from_memory_with_format(&bytes, format)
        .map_err(|error| mismatch(&path, format!("cannot decode staged artwork: {error}")))?;
    if plan.artwork.dimensions != Some((image.width(), image.height())) {
        return Err(mismatch(&path, "canonical artwork dimensions changed"));
    }
    Ok(())
}

fn relative_track<'a>(
    plan: &GroomingPlan,
    track: &'a crate::plan::TrackPlan,
) -> Result<&'a Path, ValidationError> {
    track
        .destination
        .strip_prefix(&plan.destination)
        .map_err(|_| {
            mismatch(
                &track.destination,
                "planned track is outside the album destination",
            )
        })
}

fn digest(path: &Path) -> Result<[u8; 32], ValidationError> {
    let mut file = File::open(path).map_err(|error| mismatch(path, error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| mismatch(path, error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn mismatch(path: &Path, invariant: impl Into<String>) -> ValidationError {
    ValidationError {
        path: Some(path.to_owned()),
        invariant: invariant.into(),
    }
}
