# Standalone-track and scope decisions

## First-class loose tracks

An explicitly selected loose audio file is a v0.1 input because the user's main
playlist contains many standalone tracks whose metadata and artwork matter in
car clients. This does not authorize playlist-wide or library-wide scanning.

Associate a confidently identified loose track with a real release by default.
For a lone source track, prefer a matching official single; use a studio album
only when no matching single exists, unless credible existing metadata or
identifiers establish a different origin. Ask when materially different choices
remain. If no release is defensible, keep it albumless rather than fabricate one.

## Identification fallback

For one selected loose track, optionally use `fpcalc` and AcoustID only when
identifiers, tags, filename, and duration are insufficient. Cache the lookup,
degrade gracefully when unavailable, never submit fingerprints, and do not
routinely fingerprint album tracks.

## Existing metadata fallback

When no usable MusicBrainz match exists, allow complete and coherent existing
metadata as an explicitly selected, clearly unverified fallback. If essential
fields are missing or contradictory, stop with a concise explanation rather
than adding a general tag editor.

Previously groomed albums and standalone tracks may be explicitly selected for
a later provider retry, still producing a separate result. Do not add automatic
library discovery, in-place updates, or a durable ingestion database.

## Deferred workflows

- Playlist and multi-item batch processing.
- Playlist and cue-sheet reference rewriting.
- Native splitting of cue sheets backed by one large audio image.
- Explicit in-library replacement.
- Minimal manual rescue for a liked album or track that remains unidentified;
  this is important, but follows the provider-backed v0.1 rather than turning
  initial matching into a general tag editor.
- Artwork transcoding when the only plausible cover uses a format Navidrome
  cannot consume directly.
- Concurrent source-change detection beyond staging and result validation.
- Machine-facing CLI, stable JSON schema, and non-interactive apply protocol.

The internal workflow should remain reusable for these later features without
implementing them in v0.1.
