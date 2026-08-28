# User workflow

## Accepted interaction direction

One invocation should carry an explicitly selected album directory or loose
audio file from inspection through an optional apply:

```text
music-groomer SOURCE --output OUTPUT
```

Remember a default destination root in a small user configuration file; it will
normally be the Navidrome media-library root. In the guided session, show a
visible destination-change action. Require an alternative root to exist, show
the resulting full album path and any collision, then offer to use it once or
save it as the new default. Always show and explicitly confirm the fully
resolved destination before apply.

Temporary construction is separate from this final destination: build and
validate in the operating system's temporary directory, then publish the new
result under the configured root. Ordinary filesystem permissions are enough;
the live library is not a privileged or inviolable destination.

For a file source, inspect and groom only that file; never infer that arbitrary
siblings belong to it. For a directory, recurse through ordinary subdirectories
and treat the contents as one logical release, including its ancillary files.
A directory containing one audio file is therefore a single-release source.
State the selected-source interpretation in preview.

The session should:

1. Inspect the selected directory and summarize what it found.
2. Search for plausible album metadata.
3. Continue automatically when one result is clearly best.
4. When necessary, show a small choice using recognizable information such as
   artist credit, album title, year, format, disc and track counts, and cover.
5. Build and display the exact proposed result.
6. Offer clear actions such as `Apply`, `Review choices`, `Artwork`, `Change
   destination`, and `Cancel`.
7. Apply only after affirmative confirmation.
8. Validate the groomed album and report its final path.

MusicBrainz identifiers may appear in an optional details view for provenance
or expert troubleshooting, but they must not be part of the ordinary workflow.
Artwork choices should offer a `View` action that opens the proposed image in
the user's normal image viewer when the terminal cannot display it well.

Use restrained bold and color styling to make headings, paths, selected values,
changes, warnings, and errors easy to scan. Never rely on color alone, disable
styling when output is not a terminal, and honor `NO_COLOR`.

The guided interface should consume structured inspection, candidate, and plan
values rather than contain product decisions itself. This leaves room for a
post-v0.1 machine-usable command-line mode without duplicating the workflow. Do
not commit v0.1 to a public JSON schema, machine exit semantics, or
non-interactive apply protocol.

Milestone 2 exposes the first genuine part of this flow as read-only
`music-groomer SOURCE`: inspect and summarize, with `Review files and tags` for
the complete inventory. It does not contact providers, inspect the destination,
or offer Apply yet. This is the beginning of the final guided interaction, not
a disposable diagnostic subcommand.

Milestone 3a adds automatic MusicBrainz matching after successful inspection.
Announce network work and cache hits clearly. Automatically continue on a clear
match; show at most three materially distinct choices initially, with actions to
show more, use coherent existing metadata as unverified, or cancel. Keep all
usable candidates in structured data for a future machine interface.

Offer `Refresh provider data` inside the same review session. A failed refresh
keeps the old cache and preview. If a successful refresh would materially alter
the chosen result, leave the current preview unchanged until the user accepts
the refreshed choice. Fetch archive artwork only after the metadata match is
settled.

`music-groomer --offline SOURCE` never contacts a provider and uses cache or
coherent source metadata with visible stale or unverified status. Keep cache
maintenance outside the grooming session: `music-groomer cache` reports concise
read-only status, while `music-groomer cache clear` shows the owned cache path
and size and confirms before deletion.

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

Choosing `Apply` is the explicit apply action. Its final confirmation defaults
to Yes (`[Y/n]`); declining returns to preview. An empty main-menu answer simply
redisplays the choices and never silently means Cancel.

## Ambiguity

Do not ask the user to choose between editions whose differences disappear in
the groomed result. Collapse equivalent candidates.

When candidates would produce meaningfully different track lists, credits,
dates, or artwork, present a short human-readable choice in the current
session. Provider IDs and a second command are fallback diagnostics, not user
interface.

A sole credible candidate continues automatically and remains revisable under
Review. If the only candidate is genuinely uncertain or conflicting, ask a
Yes/No confirmation rather than displaying a one-item numbered choice.

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

During Apply, show the meaningful stages without dumping routine per-file
noise: `Copying source`, `Grooming staged copy`, `Validating`, and `Publishing`.
On failure, identify the stage, affected path when known, concrete cause,
whether the source remained untouched, whether anything reached the
destination, and whether temporary data was cleaned. Never reduce these facts
to an unexplained `operation failed` message.

After a handled Apply failure and cleanup, return to the unchanged preview so
the user can review, change destination, retry, or cancel without repeating
provider matching. A retry performs fresh destination, collision, permission,
and space preflight checks. The failure report remains visible in the terminal
history. A source-change failure is the exception because it invalidates the
preview: name the changed path, state that nothing was written, and require a
fresh inspection.

Immediately before staging, compare the selected source's inspected paths,
object types, sizes, and modification times. Refuse Apply when that simple
inventory check changed. Do not hash every source file or attempt to merge
external changes.

If final validation finds that the staged result differs from the confirmed
preview, refuse publication without an override. Name the mismatched invariant,
clean the handled staging data, and return to preview; an apparently successful
write is not enough when the promised result cannot be verified.

Build and validate in the operating system's temporary directory so ordinary
failures do not leave dead album copies on persistent storage. Check available
temporary space before starting and remove temporary work after success or any
handled failure.

Check every filesystem that will hold a complete copy and block when reported
free space is clearly insufficient. If free space cannot be measured reliably,
show a warning but allow Apply; a later capacity failure remains handled and
must identify the affected stage and path.

If temporary storage and the output share a filesystem, rename the validated
album atomically into place. Otherwise, copy it through a clearly marked hidden
publication directory beside the destination, then rename that directory. Clean
it after handled failures. A later run may automatically remove an abandoned
publication directory only when its marker proves music-groomer created it.
When the destination root is next used, inspect only music-groomer's dedicated
partial area rather than scanning the library. Show each abandoned partial and
its size, then ask before removal with a guided-mode default of yes. A cleanup
failure names the path and cause but does not block a new Apply when collision
and free-space checks still pass.

A hard crash during cross-filesystem publication can still leave short-lived
output-side data. Keep this mechanism direct and testable; do not add a job
system or exotic filesystem machinery to eliminate that narrow case.
