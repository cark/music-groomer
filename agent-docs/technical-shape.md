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

Keep destination behavior separate from matching and planning. v0.1 has only a
separate-output destination policy. A later explicitly selected in-library
update can reuse the same plan while supplying different replacement and
validation behavior.

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
a database. Bound total cache size, prune expired metadata and least-recently
used entries, expose a straightforward clear operation, and make the size limit
configurable. The default maximum is 256 MiB. Test pruning with a deliberately
tiny limit and controlled time.

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

Do not rewrite playlist or cue-sheet references in v0.1. Copy those files
unchanged and warn when renamed audio may make their local references stale.
Reliable rewriting involves encodings, internal metadata, external paths, and
different cue layouts, so it requires a separately demonstrated workflow.
