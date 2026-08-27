# Development plan

This page tracks implementation. Durable product reasoning belongs in the
[decision index](open-decisions.md) and its linked pages.

## Current status

- Product and technical alignment through milestone 2 completed on 2026-08-27.
- Milestone 0 documentation baseline completed on 2026-08-27.
- Milestone 1's revised guided interaction was accepted by the user on
  2026-08-27.
- Milestone 2's decisions are documented and awaiting final scope approval;
  implementation has not been authorized.

## Working rules

- Complete and verify one milestone before building on it.
- Make small coherent commits; do not mix milestones or unrelated cleanup.
- Keep the guided workflow usable while adding adapters underneath it.
- Use temporary fixtures for automated tests.
- Do not access real source music or the live library without path-specific
  approval.
- If evidence contradicts an accepted decision, stop and realign rather than
  silently changing the product.

## Milestone 0: documentation baseline

Status: completed

Acceptance:

- `AGENTS.md` points to the self-describing wiki.
- Decision pages are focused, linked, and contain no unresolved question.
- This development plan is current.
- Documentation has no trailing whitespace or broken local links.
- Commit contains documentation only.

## Milestone 1: core and guided UX

Status: completed

Build domain values for inspection, candidates, decisions, changes, warnings,
layout, immutable plans, and apply reports. Implement deterministic matching and
layout as pure code. Drive one complete guided preview with fake album and
standalone-track data; perform no real tag writes or provider requests.

Acceptance:

- Ordinary confident matches proceed without unnecessary questions.
- Ambiguity is presented with human-readable choices, never copied identifiers.
- Summary, expanded changes, artwork choice, destination confirmation, and
  cancel behavior are demonstrable.
- Album, collaboration, compilation, multi-disc, matched-single, and unmatched
  standalone layouts have focused tests.
- User reviews the runnable fake-data interaction before milestone 2.

Review the interaction without reading music, writing files, saving settings,
or contacting providers:

```text
nix develop -c cargo run -- demo
```

The session offers an ordinary album, an ambiguous collaboration, a matched
single, and an unmatched standalone track. Its Apply and configuration-save
actions are explicitly simulated and write nothing. A destination supplied by
the user is checked only for existence, directory type, and final-path
collision. Named scenarios are also available for focused testing, but are not
part of the intended end-user workflow.

## Milestone 2: file inspection and preservation

Status: aligned, awaiting explicit implementation approval

Add the genuine read-only `music-groomer SOURCE` guided inspection backed by a
structured filesystem inventory and Lofty tag reader. Implement tag writing as
an internal preservation proof against temporary copies only; user-facing Apply
remains milestone 4. See [source inspection](decisions/source-inspection.md) and
[files, tags, and artwork](decisions/files-tags-artwork.md).

Commit tiny reproducible synthetic silent seeds for FLAC, MP3, AAC-in-M4A,
ALAC-in-M4A, Ogg Vorbis, and Opus. Copy seeds into temporary directories before
every mutation. Align the manifest's Rust requirement with the pinned Rust 1.97
development toolchain.

Acceptance:

- `music-groomer SOURCE` shows a concise styled inspection and an understandable
  full file-and-tag review without providers, destination access, or Apply.
- Inspection recursively handles one logical release, disc subdirectories,
  loose-file boundaries, mixed supported formats, ancillary and hidden files,
  symlinks, special objects, and unreadable paths according to accepted policy.
- Content detection distinguishes actual audio and image formats from misleading
  extensions and reports the exact eventual canonical rename.
- Missing or contradictory tags are structured warnings; corrupt or unsupported
  audio and preservation-blocking filesystem conditions are structured blockers.
- Artwork selection follows root-level name priority, reports ties and
  alternatives, accepts JPEG/PNG/WebP/GIF natively, and does not transcode.
- Cue-image sources block with a useful explanation; ordinary cue and playlist
  files are preserved with stale-reference warnings where applicable.
- Each claimed audio format proves intended tag changes, semantic preservation
  of unrelated tags, exact embedded-picture preservation, and valid unchanged
  audio properties on temporary fixture copies.
- A format that fails the preservation contract remains visibly unsupported.
- A successful inspection can carry warnings and exit successfully; blockers
  fail coherently without touching the source or configured destination.

## Milestone 3: providers, cache, and real matching

Status: pending

Integrate MusicBrainz and Cover Art Archive behind narrow adapters. Add the
bounded file cache and optional `fpcalc`/AcoustID fallback for one poorly
identified loose track.

Acceptance:

- MusicBrainz identification and rate limiting use a meaningful User-Agent.
- Equivalent editions collapse when they produce the same groomed result.
- Original release year and collaboration credits follow accepted policy.
- Provider failures degrade to cache or coherent existing metadata as designed.
- Cache entries are atomic, corruptible without correctness loss, refreshable,
  clearable, and pruned under the configurable 256 MiB default limit.
- AcoustID is lookup-only, optional, cached, and never used routinely on albums.
- Core provider tests use fakes; narrowly scoped live smoke tests use no real
  music or library paths.

## Milestone 4: safe apply and validation

Status: pending

Apply a confirmed immutable plan to temporary staging, validate it, and publish
the separate result without overwriting collisions.

Acceptance:

- Source bytes and paths remain unchanged.
- Handled failures clean temporary and publication data.
- Same-filesystem rename and forced cross-filesystem publication paths are both
  covered without requiring a special test mount.
- Abandoned publication cleanup verifies the ownership marker.
- Validation re-reads tags, audio properties, embedded artwork, sidecar artwork,
  filenames, ancillary files, and destination layout.
- Apply reports its copy, grooming, validation, and publication stages. Failures
  identify the stage, path and cause where known, source and destination status,
  and cleanup outcome.
- Interruption and injected failure points leave no final partial album.
- Full format, core, CLI, provider-fake, filesystem, and apply suites pass.

## Milestone 5: real candidate exercise

Status: blocked on explicit path and approval

Use Ten Years After - Evolution only after the user supplies its path and grants
read access. Present a real preview first. Apply only after separate explicit
approval and only to the configured destination root. Do not access or modify
the live library before that approval.

## Standard verification

Run from the repository through the existing development environment:

```text
nix develop -c cargo fmt --all -- --check
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo test --all-targets --all-features
```

Add focused tests and checks when a milestone introduces behavior not covered by
these commands.

## Known conditional risks

- Lofty behavior must be proven independently for every accepted format.
- MusicBrainz may expose candidate patterns that require revisiting match
  evidence without changing the guided interaction.
- AcoustID lookup depends on `fpcalc`, service availability, and client-key
  configuration; its absence must not block other workflows.
- Opening artwork in a desktop viewer must remain optional and testable without
  a graphical session.
