# Source-inspection decisions

## Milestone 2 interaction

Milestone 2 adds genuine read-only inspection to the guided command:

```text
music-groomer SOURCE
```

It does not add user-facing Apply, provider matching, or destination access.
The default view summarizes the source interpretation, audio count and formats,
duration, common album fields, disc and track coverage, artwork choice,
ancillary count, warnings, and blockers. `Review files and tags` exposes the
complete per-file inventory and exact detected discrepancies.

Inspection returns structured values which the human renderer consumes. Keep
warnings structured as result data so a later machine-facing interface can
report them without scraping prose. A successful inspection may contain
warnings and exits successfully; a blocker fails the inspection. Stable JSON,
detailed exit-code semantics, and non-interactive Apply remain post-v0.1.

## Source boundaries

Selecting one file inspects only that file. Selecting a directory recursively
inspects all ordinary subdirectories without following symbolic links. Audio
at any depth belongs to the selected item, and ordinary non-audio files are
ancillary. Hidden ordinary files are not treated as disposable junk.

One selected directory must represent one logical release. Disc subdirectories
are valid, but clear evidence of several unrelated releases blocks rather than
silently becoming batch processing. A directory containing no supported audio
cannot be groomed.

Mixed supported audio formats are allowed and visibly warned about; grooming
does not transcode audio. Symlinks are neither followed nor copied and produce
warnings. Sockets, devices, and other special objects block. An unreadable
audio or ancillary file also blocks and reports its path and operating-system
error.

## Recognition and severity

Recognize supported audio and image formats from their contents rather than
trusting filename extensions. A certainly recognized supported file with a
wrong extension remains usable: inspection warns and the eventual preview
shows the canonical extension change. Unknown, damaged, or unsupported audio
blocks the selected item. A genuinely non-audio file remains ancillary even
when its type is unknown. A probable unsupported image is preserved as
ancillary but cannot be the canonical cover.

Probe inexpensive image signatures before invoking the audio parser. Content
recognized as an image must bypass audio parsing; large scans otherwise risk
expensive false MPEG detection. An image-like filename alone is not enough to
skip audio recognition, so audio with the wrong extension remains usable.

Missing or contradictory metadata remains inspectable as structured warnings;
provider matching in milestone 3 may repair it. Only unreadable or corrupt
audio, unsupported audio, special filesystem objects, and other conditions
that prevent safe preservation block at inspection time.

For MP4-family input, accept supported AAC or ALAC audio only when the container
has no video track. An audio-plus-video MP4 blocks in v0.1 with a clear
explanation; support can be reconsidered after testing Navidrome and the intended
music clients. Use the narrow MP4 parser only to establish this track shape.

A readable probable image rejected by the canonical image decoder is preserved
with a warning and cannot be chosen as canonical artwork. Only an actual read
failure blocks preservation.

When deciding whether a directory appears to contain several releases, compare
album titles case-insensitively after collapsing insignificant whitespace.
Report those cosmetic variations as a warning, while keeping punctuation and
wording significant enough to expose genuinely different albums.

## Cue sheets

Navidrome's one-file-per-track model means a cue sheet plus one large audio
image cannot produce polished client-visible tracks without splitting the
audio. Detect and block that probable shape with a clear explanation and a
recommendation to split it externally. Native cue-image splitting is deferred.

A cue sheet accompanying already separated tracks is ordinary ancillary data.
Copy it unchanged later, and warn when planned audio renames may leave its
references stale. Detect structural `TRACK` declarations from raw bytes without
assuming the cue sheet is UTF-8, and preserve the original bytes. Failure to
read a probable cue sheet blocks inspection.

## Source stability

v0.1 assumes the explicitly selected source is stable during its short guided
session. Do not add whole-file hashes or size-and-time snapshot tracking.
Apply will copy into temporary staging and validate the staged result; handled
read, copy, or validation failures stop publication and leave the source
untouched. More elaborate concurrent-change detection is a possible later
hardening measure if real use demonstrates the need.

## Progress and diagnostics

Interactive inspection must report work immediately and update a transient
status with the current ordinary file. A slow directory must never resemble a
stalled program merely because the complete inspection summary is not ready.

`--diagnostics` explicitly enables one detailed human-readable log; ordinary
runs perform no diagnostic file I/O. Store it at `diagnostics.log` in the
platform application-state directory, falling back to the application-cache
directory where no state directory exists. Replace it on each diagnostic run,
hold an exclusive lock for the run, show its resolved path, and fail clearly if
it cannot be created, replaced, or locked.

Record owned phase and per-file spans with elapsed times, byte sizes,
classification, outcomes, errors, and full local paths. Do not record tag
contents, fingerprints, provider bodies, or credentials. Keep dependency logs
filtered out by default. `--diagnostics=audio` additionally admits Lofty and
mp4parse trace events for parser-level investigation, bridging their standard
Rust `log` records into the tracing subscriber, while continuing to exclude
provider and HTTP dependencies. Tests inject temporary paths and never write
to the user's real application directories.
