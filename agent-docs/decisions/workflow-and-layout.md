# Workflow and layout decisions

## Guided interaction

Use one simple guided terminal session for inspection, human-readable candidate
choices, exact preview, destination confirmation, and explicit apply. Provider
identifiers stay in expanded details. Artwork can be opened in the normal image
viewer. Do not require identifiers to be copied between commands.

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

Remember a default separate-output root in a small user configuration file and
allow a one-run override. In guided mode, always show and confirm the fully
resolved destination before applying.

v0.1 always creates a separate result. Explicitly updating an existing live
library item is the next destination workflow, not part of v0.1.
