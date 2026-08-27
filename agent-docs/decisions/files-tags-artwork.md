# Files, tags, and artwork decisions

## Audio formats and preservation proof

Support FLAC, MP3, AAC-in-M4A, ALAC-in-M4A, Ogg Vorbis, and Opus only after
format-specific fixtures prove intended tag changes and preservation behavior.
Commit tiny synthetic silent seed files for each format, together with a
reproducible generation note or script. Mutation tests copy those seeds into
temporary directories and never alter the committed files.

For every claimed format, prove that planned groomed fields are written,
unrelated tags are semantically preserved, embedded pictures retain their exact
bytes, MIME type, picture type, and description, and available codec, duration,
sample-rate, and channel properties remain valid. Raw tag-block or whole-file
byte identity is not required. The proof includes lyrics, unrelated text,
multiple embedded pictures, and the legacy ID3v1 container in MP3 fixtures. If
a format cannot meet this contract safely, keep it visibly unsupported instead
of silently losing data.

Mixed supported formats within one logical release are accepted with a warning
and are not transcoded. Detect actual formats from content. When a supported
audio file has a misleading extension, show the exact eventual rename to its
canonical `.flac`, `.mp3`, `.m4a`, `.ogg`, or `.opus` extension.
MP4 files containing both audio and video tracks are unsupported in v0.1. Revisit
that boundary only with evidence from Navidrome and the intended clients.

## Source preservation

Copy every ordinary ancillary file and subdirectory, including hidden files,
while preserving contents, relative paths, and ordinary Unix permission bits
where practical. Do not preserve ownership, timestamps, ACLs, or extended
attributes. New files belong to the invoking user; destination permissions and
host configuration are responsible for Navidrome access.

Do not follow or copy symbolic links; reject unusual objects such as sockets and
devices. An unreadable ancillary file blocks rather than disappearing from the
result. Preview every skipped, renamed, relocated, replaced, or rewritten item.

Copy playlists and ordinary cue sheets unchanged. When audio names change, warn
that `.m3u`, `.m3u8`, or `.cue` references may be stale. Rewriting them is
explicitly deferred because encodings, external paths, metadata, and cue
structures make it a separate feature. Cue-image releases follow the blocking
rule in [source inspection](source-inspection.md).

Selecting one audio file selects only that file. Directory selection follows
the recursive, one-logical-release rules in source inspection.

## Active metadata

Groom title, track artist and constituent artists, album, album artist and
constituent album artists, track and disc positions and totals, compilation
status, canonical date, and accepted MusicBrainz identifiers.

Write identifiers for artists, album artists, confidently mapped recordings,
and the release group when confidently known. Absence of a replacement ID in a
plan preserves the existing value. v0.1 deliberately never adds or changes the
exact release ID: preserve an existing one unchanged, because this workflow is
not trying to identify a pressing or catalogue edition.

Compilation status is explicit groomed metadata. Set it for a genuine
compilation and clear an incorrect compilation flag for an ordinary or
collaboration release.

Preserve existing genre, ReplayGain, lyrics, embedded pictures, and other
unrelated useful tags. Do not fetch or normalize lyrics. Do not fetch, infer,
split, normalize, or replace genre in v0.1.

Use a natural singular `ARTIST`/`ALBUMARTIST` display credit plus plural
`ARTISTS`/`ALBUMARTISTS` constituent values where the format supports them. A
groomed MP3 may be upgraded to ID3v2.4 so genuine multi-values can be represented
correctly; the source remains untouched. Legacy tag-container behavior is an
implementation detail that must satisfy the preservation contract above.

## Collaborations and compilations

Represent every credited collaborator in both the natural display credit and
separate artist values where supported. A duo such as NHOP and Kenny Drew has
both album artists. Never convert a collaboration into `Various Artists`; that
label is for a genuine various-artists compilation.

## Artwork

Do not resize, upscale, recompress, or reject source artwork based on dimensions.
Preserve all source artwork. A recognizable source front is canonical by
default, with the Cover Art Archive front offered visibly as an alternative.

Choose source front-cover candidates from the release root using the
case-insensitive priority `cover`, then `folder`, `front`, then `albumart`.
Never promote a booklet, back, or disc image merely because it is larger. Ask
when candidates tie at the same priority, show the selected image and plausible
alternatives, and preserve every original image.

JPEG, PNG, WebP, and GIF are eligible canonical sidecar formats. Detect their
real format from content and publish the selected source image natively as
`cover.<correct extension>`, showing any source-name or extension correction in
the expanded review. Do not resize, recompress, or transcode it. A probable
image in another format is preserved with a warning but cannot be selected as
canonical artwork in v0.1.

Use the Rust `image` decoder as the canonical eligibility check. A readable
probable image that it cannot decode remains preserved ancillary data with a
warning; it does not block grooming. Failure to read the file at all blocks
because preservation cannot be proven.

When no source cover exists, use the archive's 1200-pixel front derivative. Do
not invent an image-quality score or download backs, booklets, or scan
collections.

If a selected replacement conflicts with root-level source cover files, put the
canonical image at `cover.<native extension>` and preserve displaced originals
byte-for-byte under `original-artwork/`. Show the relocation in preview.
