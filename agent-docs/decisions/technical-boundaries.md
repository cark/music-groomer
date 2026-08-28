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

Keep milestone 3a provider work sequential. Visible retries have a hard
60-second total deadline and bounded requests. Leave `Ctrl-C` under the
operating system's normal unconditional termination behavior rather than
installing a cancellation handler. Apply interruption is a separate milestone
4 safety decision.

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
configurable 256 MiB default maximum and prune least-recently used entries.
Stale metadata remains eligible as a provider-unavailable fallback until the
size bound evicts it. Test eviction under a tiny deterministic limit.

Allow a per-invocation exact cache-directory override for isolated smoke tests.
Apply the override uniformly to grooming and every cache command. Refuse to
claim or clear a non-empty directory without music-groomer's ownership marker;
the test harness remains responsible for its temporary directory's lifecycle.

## Command-line boundary

Use Clap at the executable boundary for familiar help, version, subcommand,
validation, and error behavior. Keep Clap types out of the core workflow.
`music-groomer SOURCE` remains the primary guided form; `cache` is a visible
maintenance subcommand whose default action is read-only status. Keep the
milestone-only `demo` command available but hidden from ordinary help.

Use `tracing` spans and events as the library-side diagnostics boundary, with
the executable owning the subscriber and diagnostic-file lifecycle. Do not
send logs directly to stdout or stderr during the guided interface: all normal
terminal output continues through semantic UI primitives. The v0.1 diagnostic
subscriber is explicit, synchronous, human-readable, and filtered to
application-owned targets; structured JSON and always-on logging are deferred.
The explicit `audio` diagnostic scope may additionally admit only the selected
tag and container-parser targets; it is not an unrestricted dependency-log
switch.

## Apply and cleanup

Build and validate in the operating system's temporary directory after a space
preflight. Clean temporary work after success and handled failure. Rename
atomically when output shares the filesystem; otherwise copy through a marked
hidden publication directory beside the destination and then rename it.

Check each filesystem that must hold a complete copy, including a small safety
margin. Clearly insufficient reported space blocks before copying. Failure to
obtain a meaningful free-space measurement is non-blocking: warn visibly and
let the real staged writes enforce capacity, retaining the normal clean failure
report if they run out of space.

The final rename must also refuse replacement atomically: a destination created
after the earlier collision check must survive untouched. Keep the
platform-specific code to one narrow publication primitive backed by
`rustix`'s exclusive rename operation on Linux and macOS and the equivalent
native no-replace rename on Windows. Linux, macOS, and Windows must all retain
the guarantee from v0.1 even when a platform cannot be exercised locally; do
not weaken it with a check-then-rename fallback. Ordinary `Ctrl-C` termination
may occur before or after this syscall, but cannot expose a half-renamed
directory.

Continuously check that portability with one minimal GitHub Actions matrix on
standard Linux, macOS, and Windows runners. Run the ordinary locked test suite
only; the explicitly ignored live provider smoke tests remain opt-in. Add no CI
caching, artifacts, coverage, badges, packaging, or release automation in this
milestone.

Validation is the final proof that the staged result matches the confirmed
preview. Any promised invariant that fails to round-trip blocks publication:
v0.1 has no Apply-anyway override. Report the exact mismatch, clean handled
staging data, and return to the unchanged preview so the user may retry or
cancel. This guard primarily catches format-specific writer surprises,
dependency regressions, filesystem anomalies, and implementation mistakes
after individual operations appeared successful.

Clean output-side publication data after handled failures. On a later run,
inspect only music-groomer's dedicated partial area under the destination root,
not the library tree. If a marked abandoned publication is found, show its size
and ask before removing it, defaulting to yes in the guided interface. Remove
only after verifying its ownership marker. If removal fails, report the exact
path and cause but allow a new Apply when its own collision and free-space
checks still pass. Prefer simple, testable behavior over a job system or exotic
filesystem machinery.
