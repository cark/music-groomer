# Files, tags, and artwork decisions

## Audio formats

Support FLAC, MP3, M4A, Ogg Vorbis, and Opus only after format-specific fixtures
prove intended tag changes and preservation behavior.

## Source preservation

Copy ordinary ancillary files and subdirectories while preserving relative
paths. Do not follow symbolic links; reject unusual objects such as sockets and
devices. Preview every skipped, renamed, relocated, replaced, or rewritten item.

Copy playlists and cue sheets unchanged. When audio names change, warn that
`.m3u`, `.m3u8`, or `.cue` references may be stale. Rewriting them is explicitly
deferred because encodings, external paths, metadata, and cue structures make it
a separate feature.

Selecting one audio file selects only that file. Selecting a directory with one
audio file selects that track plus the directory's ordinary ancillary contents.

## Active metadata

Groom title, track artist and constituent artists, album, album artist and
constituent album artists, track and disc positions and totals, compilation
status, canonical date, and accepted MusicBrainz identifiers.

Write identifiers for artists, album artists, confidently mapped recordings,
and the release group. Write a specific release ID only when the exact release
is genuinely known, not when an equivalent edition merely supplied metadata.

Preserve existing genre, ReplayGain, lyrics, embedded pictures, and other
unrelated useful tags. Do not fetch or normalize lyrics. Do not fetch, infer,
split, normalize, or replace genre in v0.1.

## Collaborations and compilations

Represent every credited collaborator in both the natural display credit and
separate artist values where supported. A duo such as NHOP and Kenny Drew has
both album artists. Never convert a collaboration into `Various Artists`; that
label is for a genuine various-artists compilation.

## Artwork

Do not resize, upscale, recompress, or reject source artwork based on dimensions.
Preserve all source artwork. A recognizable source front is canonical by
default, with the Cover Art Archive front offered visibly as an alternative.

When no source cover exists, use the archive's 1200-pixel front derivative. Do
not invent an image-quality score or download backs, booklets, or scan
collections.

If a selected replacement conflicts with root-level source cover files, put the
canonical image at `cover.<native extension>` and preserve displaced originals
byte-for-byte under `original-artwork/`. Show the relocation in preview.
