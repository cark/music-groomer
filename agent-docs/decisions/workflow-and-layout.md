# Workflow and layout decisions

## Guided interaction

Use one simple guided terminal session for inspection, human-readable candidate
choices, exact preview, destination confirmation, and explicit apply. Provider
identifiers stay in expanded details. Artwork can be opened in the normal image
viewer. Do not require identifiers to be copied between commands.

Use restrained bold and color styling for scanability while retaining textual
labels and symbols. Disable styling for non-terminal output and when `NO_COLOR`
is set. After the explicit Apply action, default final confirmation to Yes.
Pressing Enter at the main action menu merely redisplays it.

Process one explicitly selected item per v0.1 session. Playlist and multi-item
batch processing are deferred until single-item behavior is proven.

## Date

Use the album or single's original release year for `DATE` and destination
layout. A later edition can supply a matching track list without replacing that
year. For compilations, use the compilation's original year rather than its
songs' individual years.

## Release layout

For a single-disc album:

```text
<album artist>/<year> - <album>/<track> - <title>.<extension>
```

For a multi-disc album:

```text
<album artist>/<year> - <album>/<disc>-<track> - <title>.<extension>
```

Use the complete credited album-artist display string for collaborations and
`Various Artists` only for genuine compilations. Preserve Unicode, sanitize
unsafe path characters conservatively, and treat collisions as errors.

For a matched single:

```text
<artist>/<year> - <single title>/01 - <track title>.<extension>
```

For an unmatched albumless track:

```text
<artist>/Standalone Tracks/<title>/<title>.<extension>
```

Do not invent year, album, track number, or provider identifier to fill a path.

## Output destination

Remember a default destination root in a small user configuration file. It will
normally be the Navidrome media-library root. Make `Change destination` visible
inside the guided preview. An alternative root must already exist; after
showing the resulting album path and checking for collision, offer `Use once`,
`Use and save as default`, or `Go back`. Always confirm the fully resolved
destination before applying. When changing an existing destination, show the
current root as the bracketed prompt default; Enter or explicitly repeating
that root returns to the exact preview without a save question. Do not use a
special `c` cancellation value. If no destination is configured, Enter returns
to the still-live metadata preview; it cannot advance to an exact plan until a
valid destination is selected.

Temporary staging belongs in the operating system's temporary directory and is
not the final output. v0.1 creates a new result under the destination root and
refuses collisions. Explicitly updating or replacing an existing live-library
item is the next destination workflow, not part of v0.1.
