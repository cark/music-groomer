use std::io::Read;

pub(super) fn contains_video(reader: &mut impl Read) -> Result<bool, mp4parse::Error> {
    let context = mp4parse::read_mp4(reader)?;
    Ok(context
        .tracks
        .iter()
        .any(|track| track.track_type == mp4parse::TrackType::Video))
}
