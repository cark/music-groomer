# User workflow

## Accepted interaction direction

One invocation should carry an explicitly selected album directory or loose
audio file from inspection through an optional apply:

```text
music-groomer SOURCE --output OUTPUT
```

Remember a default separate-output root in a small user configuration file so
later guided runs may omit `--output`. Allow a per-run override. In human mode,
always show and explicitly confirm the fully resolved destination before apply,
even when it came from saved configuration.

For a file source, inspect and groom only that file. For a directory containing
one audio file, treat it as a single release source and preserve its ordinary
ancillary contents. State that interpretation in preview; never infer that
arbitrary siblings belong to a selected file.

The session should:

1. Inspect the selected directory and summarize what it found.
2. Search for plausible album metadata.
3. Continue automatically when one result is clearly best.
4. When necessary, show a small choice using recognizable information such as
   artist credit, album title, year, format, disc and track counts, and cover.
5. Build and display the exact proposed result.
6. Offer clear actions such as `Apply`, `Review choices`, and `Cancel`.
7. Apply only after affirmative confirmation.
8. Validate the groomed album and report its final path.

MusicBrainz identifiers may appear in an optional details view for provenance
or expert troubleshooting, but they must not be part of the ordinary workflow.
Artwork choices should offer a `View` action that opens the proposed image in
the user's normal image viewer when the terminal cannot display it well.

The guided interface should consume structured inspection, candidate, and plan
values rather than contain product decisions itself. This leaves room for a
post-v0.1 machine-usable command-line mode without duplicating the workflow. Do
not commit v0.1 to a public JSON schema, machine exit semantics, or
non-interactive apply protocol.

When added later, a machine-facing mode should use stable structured output and
keep progress or diagnostics separate from result data. It must never depend on
an interactive confirmation; applying from that mode will require an explicit
machine-usable authorization mechanism.

## Preview

The default summary should emphasize decisions rather than dump every tag:

- selected album and why the match is convincing;
- destination directory;
- artwork source and appearance;
- concise counts of changed and unchanged tracks;
- warnings and unresolved questions.

An expanded review should show every tag and filename change. The final apply
confirmation must refer to the exact in-memory plan being shown; it must not
perform a fresh search after confirmation.

## Ambiguity

Do not ask the user to choose between editions whose differences disappear in
the groomed result. Collapse equivalent candidates.

When candidates would produce meaningfully different track lists, credits,
dates, or artwork, present a short human-readable choice in the current
session. Provider IDs and a second command are fallback diagnostics, not user
interface.

## Provider-unavailable fallback

If no usable provider match is available, offer to use existing source metadata
only when the album is complete and internally coherent across album, artist,
title, disc, and track fields. Mark the result clearly as not verified against
MusicBrainz. If essential metadata is missing or contradictory, stop with a
concise explanation rather than turning v0.1 into a general tag editor.

Keep the inspection and matching flow reusable so an explicitly selected,
previously groomed album or standalone track can be retried against providers
later. This does not authorize in-place library mutation, playlist-wide
processing, or automatic library scanning.

## Interrupted or failed apply

Build and validate in the operating system's temporary directory so ordinary
failures do not leave dead album copies on persistent storage. Check available
temporary space before starting and remove temporary work after success or any
handled failure.

If temporary storage and the output share a filesystem, rename the validated
album atomically into place. Otherwise, copy it through a clearly marked hidden
publication directory beside the destination, then rename that directory. Clean
it after handled failures. A later run may automatically remove an abandoned
publication directory only when its marker proves music-groomer created it.

A hard crash during cross-filesystem publication can still leave short-lived
output-side data. Keep this mechanism direct and testable; do not add a job
system or exotic filesystem machinery to eliminate that narrow case.
