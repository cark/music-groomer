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
Preserve the track's real position and release total: one selected track from a
two-track single is tagged `1/2`, not normalized to `1/1`.

## Identification fallback

The accepted fingerprinting, provider, cache, matching, and presentation policy
is recorded in [Milestone 3b loose-track identification](milestone-3b-identification.md).

## Existing metadata fallback

When no usable MusicBrainz match exists, allow complete and coherent existing
metadata as an explicitly selected, clearly unverified fallback. If essential
fields are missing or contradictory, stop with a concise explanation rather
than adding a general tag editor.

Previously groomed albums and standalone tracks may be explicitly selected for
a later provider retry, still producing a separate result. Do not add automatic
library discovery, in-place updates, or a durable ingestion database.

Completing or rebuilding an existing release is an important post-v0.1
workflow. A library assembled from loose tracks may encounter the remaining
tracks of a release individually, or may eventually reconstruct a complete
album piece by piece. v0.1 still refuses the existing release-directory
collision rather than attempting a non-atomic merge. Explain that specific
limitation at the collision instead of presenting it as an unexplained path
error. A later design must verify release identity, existing track and artwork
compatibility, per-file collisions, and interruption behavior.

## Deferred workflows

- Playlist and multi-item batch processing.
- Playlist and cue-sheet reference rewriting.
- Native splitting of cue sheets backed by one large audio image.
- Explicit in-library replacement.
- Minimal manual rescue for a liked album or track that remains unidentified;
  this is important, but follows the provider-backed v0.1 rather than turning
  initial matching into a general tag editor.
- Manual provider-search refinement with user-edited artist, album, or title
  terms. This could recover a match when poor source tags produce a bad
  automatic query, but is deferred until real misses demonstrate the need.
- Artwork transcoding when the only plausible cover uses a format Navidrome
  cannot consume directly.
- Concurrent source-change detection beyond staging and result validation.
- Machine-facing CLI, stable JSON schema, and non-interactive apply protocol.

The internal workflow should remain reusable for these later features without
implementing them in v0.1.
