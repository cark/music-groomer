# Technical shape

## Accepted core boundaries

Represent the sequential workflow with typed values rather than a workflow
framework:

```text
Inspection -> Match decision -> Grooming plan -> Apply report
```

Core decisions should operate on in-memory values and distinguish an album from
a genuinely standalone track rather than assuming every input is an album:

- matching and confidence;
- track mapping;
- metadata comparison;
- layout generation and sanitization;
- warnings and the final immutable plan.

## Match confidence

Use deterministic, explainable evidence. Existing MusicBrainz identifiers are
strongest; disc and track structure plus unique positions are hard evidence;
durations are strong support; normalized artist, album, and title similarity are
supporting evidence; dates are weaker. Artwork never proves a metadata match.

Auto-select only when every track maps uniquely, evidence is strong, and the
best candidate clearly exceeds the runner-up. Otherwise present materially
different alternatives and their reasons. Do not use machine learning or show a
confidence number without an explanation.

Keep destination behavior separate from matching and planning. v0.1 creates a
new result under the configured destination root, normally the media library,
and refuses collisions. A later explicitly selected in-library update can reuse
the same plan while supplying replacement and validation behavior.

Keep narrow adapters around:

- audio tag and property reading/writing;
- metadata and artwork providers;
- terminal interaction;
- filesystem application.

Human and future machine-facing interfaces should both consume the same
structured workflow results. Terminal prompts and rendering must not own
matching, metadata, or apply decisions.

Do not mirror all of `std::fs` behind a broad trait. Test pure decisions with
values and fakes, and test real filesystem behavior in temporary directories.

## Provider and metadata components

- MusicBrainz for releases, recordings, credits, dates, and identifiers.
- Cover Art Archive for a curated front image.
- Lofty for format-aware tag access, conditional on fixture tests proving
  intended changes and preservation for every claimed format.

This is the complete v0.1 online provider set. Do not add Discogs, Spotify,
Apple Music, Last.fm, or another metadata source until real misses demonstrate a
need. Keep provider responses mapped into small local domain values so client
library choices do not leak into the core.

After a non-blocking local inspection, the guided workflow searches MusicBrainz
automatically and visibly announces potentially slow network work. Existing
identifiers, tags, filenames, positions, and durations form both the query and
the explainable ranking evidence. A clear result is selected automatically;
the human view initially shows at most three materially distinct ambiguous
results and can show more. Keep every usable candidate in structured workflow
data so a future machine interface is not constrained by terminal presentation.

Fetch Cover Art Archive data only after the metadata match is settled. Use the
release-group 1200-pixel front rather than choosing an edition-specific scan.
A provider match without usable source or archive artwork remains valid with a
prominent warning.

Use direct synchronous HTTP through `ureq` for both providers. Do not add an
async runtime or a MusicBrainz-specific client when caching, retry behavior, and
Cover Art Archive access already require a small application-owned adapter. Do
not invent a general HTTP trait; fake the narrow metadata and artwork provider
boundaries in core tests. Identify requests as
`music-groomer/<version> (https://github.com/cark/music-groomer)`.

For one explicitly selected loose track, v0.1 may use `fpcalc` and an AcoustID
lookup only when MusicBrainz identifiers, existing tags, filename, and duration
do not produce sufficient confidence. Keep this optional and behind narrow
process and provider boundaries. Do not fingerprint albums routinely and do not
submit fingerprints to AcoustID.

Provider matching must not be fused with first-time ingestion. The same core
inspection and planning operations should be able to reconsider a previously
groomed album or standalone track later, while keeping playlist-wide and
live-library update workflows out of v0.1.

## Provider cache

External metadata and artwork requests can be slow and rate limited. Reuse
responses throughout one session, and keep a small persistent local cache so
repeating a preview or apply does not immediately refetch identical data.

This cache is an implementation detail, not workflow persistence: it contains
provider responses and fetch metadata, not jobs, source archives, or apply
state. Cache freshness, refresh behavior, and corruption handling must be
explicit. A missing or damaged cache should cost performance, not correctness.

Use ordinary atomically written files in the platform user-cache directory, not
a database. Bound total cache size, prune least-recently used entries, expose a
straightforward clear operation, and make the size limit configurable. The
default maximum is 256 MiB. Test pruning with a deliberately tiny limit and
controlled time. Staleness alone does not delete metadata because stale data is
the intended provider-unavailable fallback; the size bound prevents it from
growing without limit.

A metadata entry is fresh for a named, documented 30-day constant in v0.1. A
fresh hit bypasses MusicBrainz completely. Refresh stale metadata; if that
request fails, use the stale entry with a visible warning. Reuse cached artwork
without routine redownloads. Explicit refresh bypasses freshness, and a corrupt
entry behaves as a cache miss. Do not expose freshness as configuration until
real use demonstrates a need.

Transient provider failures retry sequentially for at most 60 seconds total.
Announce the retry window and every delay, honor a reasonable provider
`Retry-After`, and enforce hard connection and response timeouts. Do not install
a custom `Ctrl-C` handler in milestone 3a: the operating system's ordinary
interrupt must always terminate the program, so a workflow bug cannot hold the
user's terminal. Reaching the retry deadline returns to available cache or
local-metadata fallbacks.

Refresh is transactional: retain the old response and active preview until a
replacement has been fetched and parsed successfully, then atomically replace
the cache entry. A valid refreshed response may enter the cache without
silently replacing a materially different active match. Offline mode performs
no network requests and may use stale cache entries with clear status.

`music-groomer cache` reports cache location, total usage and limit, metadata
fresh/stale counts, artwork count and size, and damaged-entry count without
mutating anything. `music-groomer cache clear` confirms before deleting only
music-groomer's cache. Automatic pruning occurs on writes, never merely because
status was requested.

`--cache-dir PATH` selects an exact cache directory for the whole invocation,
primarily for smoke tests and other isolated machine runs. Normal grooming,
offline mode, status, and clearing all use the selected cache. The caller owns
temporary-directory cleanup. A missing or empty directory can become a marked
music-groomer cache; a non-empty unmarked directory must never be claimed or
cleared.

## Apply transaction

1. Refuse existing final and partial destinations.
2. Check available space and create staging in the operating system's temporary
   directory.
3. Copy audio files; never open source files for writing.
4. Copy ordinary ancillary files and directories without following symbolic
   links; reject unusual filesystem objects.
5. Modify only staged copies and write the sidecar.
6. Re-read and validate the planned result, including preservation of embedded
   pictures.
7. If staging and output share a filesystem, rename staging to the final path.
   Otherwise, copy through a marked hidden publication directory on the output
   filesystem and rename it only when complete.
8. Remove temporary data after success and all handled failures. On later runs,
   remove abandoned output-side publication directories only after verifying
   their ownership marker.

There is no database, queue, worker, or automatic background retry.

## Deferred reference rewriting

Do not rewrite playlist or ordinary cue-sheet references in v0.1. Copy those
files unchanged and warn when renamed audio may make their local references
stale. A probable cue sheet backed by one large audio image blocks because
Navidrome cannot expose its virtual tracks; native splitting is deferred.
Reliable rewriting or splitting involves encodings, internal metadata, external
paths, cue layouts, and audio transformation, so it requires a separately
demonstrated workflow.
