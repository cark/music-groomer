# Technical-boundary decisions

## Core

Use plain typed values through `Inspection -> Match decision -> Grooming plan ->
Apply report`. Keep matching, layout, comparison, and warnings pure. Put narrow
boundaries around tag I/O, providers, terminal interaction, process execution,
and filesystem application. Add neither a workflow framework nor a broad
filesystem abstraction mirroring `std::fs`.

## Rust source organization

Keep Rust source files on the smallish side without imposing a rigid line
limit. A file should usually center on one functionally important struct or
concept, accompanied by its tightly coupled error type, small enums, helpers,
and focused tests.

Split fixtures, rendering, or another distinct responsibility when they make a
file difficult to navigate. Do not mechanically create one file per tiny type:
cohesion is the goal, not maximum fragmentation. Pragmatic exceptions are
welcome when separating tightly coupled code would make the flow harder to
understand.

## Rust toolchain policy

Target the current stable Rust supplied by the pinned Nix development flake and
raise `rust-version` with that pin rather than carrying compatibility for older
compilers without a demonstrated user. Milestone 2 will align the manifest with
the currently pinned Rust 1.97 toolchain. Current language and library features
are welcome when they make the code clearer.

## Metadata and providers

Start with current Lofty 0.25 for tag access, conditional on fixture tests
proving preservation for every claimed format. Do not introduce a second tag
stack speculatively. Use MusicBrainz for metadata and identities, Cover Art
Archive for fronts, and AcoustID only for the scoped loose-track fallback. Add
no other online metadata provider in v0.1.

Represent inspection warnings and blockers as typed data, not terminal prose.
The human interface renders them with clear text and restrained styling; a
future machine interface can serialize the same values and receive warnings on
successful results.

Keep MusicBrainz behind a narrow adapter. A small implementation evaluation may
choose a Rust client crate or direct HTTP without changing core behavior.

## Matching

Use deterministic evidence ordered by reliability: existing MusicBrainz IDs;
disc, track, and position structure; durations; normalized textual metadata;
then dates. Artwork is not match evidence. Auto-select only with unique complete
mapping, strong evidence, and a clear lead. Otherwise explain the meaningful
alternatives. Do not use machine learning or unexplained confidence numbers.

## Cache

Use ordinary atomically written files in the platform user-cache directory for
provider JSON and artwork. Do not use a database. Treat damage as a miss, allow
refresh and clearing, and store no workflow or source data. Enforce a
configurable 256 MiB default maximum, pruning expired and least-recently used
entries. Test eviction under a tiny deterministic limit.

## Apply and cleanup

Build and validate in the operating system's temporary directory after a space
preflight. Clean temporary work after success and handled failure. Rename
atomically when output shares the filesystem; otherwise copy through a marked
hidden publication directory beside the destination and then rename it.

Clean output-side publication data after handled failures. On a later run,
remove an abandoned directory only after verifying its ownership marker. Prefer
simple, testable behavior over a job system or exotic filesystem machinery.
