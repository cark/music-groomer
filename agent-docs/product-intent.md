# Product intent

## Accepted direction

music-groomer should make an ordinary album or standalone track easy to prepare
for Navidrome:

- correct artist, album, disc, track number, and track title metadata;
- sensible dates and album-artist metadata;
- attractive, correctly associated album artwork;
- predictable filenames and directory layout;
- good treatment of collaborations, compilations, and multi-disc albums.

The normal input is one album directory or one loose audio file explicitly
selected by the user. The normal output is a separate groomed album or track
suitable for later placement in the library. The source remains untouched.

Standalone tracks are part of v0.1 because the user's main listening playlist
contains many loose tracks and their artist, title, artwork, and presentation in
car clients matter. This does not authorize scanning or rewriting the playlist
or complete library.

Keep acoustic identification narrow: it is an optional fallback for one
selected loose track, not a bulk fingerprinting pipeline or submission service.

Process one explicitly selected item per guided v0.1 session: either one album
directory or one loose track. Playlist and multi-item batch processing are
deliberately deferred until the single-item workflow is proven.

v0.1 always produces a separate groomed result. Updating an explicitly selected
album or track already inside the live library is the next distinct destination
workflow after separate-output grooming is proven; do not mix its replacement
safety into v0.1.

Selecting one audio file selects only that file; arbitrary siblings do not
become source material. Selecting a directory containing one audio file selects
the single together with that directory's ordinary ancillary contents.

The product should be mostly automatic for ordinary albums. Uncertainty should
be presented in human terms and corrected inside the same guided session. The
user should not have to copy provider identifiers between commands.

## Product principles

- Preview before writing, within the same session.
- Applying always requires a clear, explicit confirmation.
- Show meaningful changes and uncertainty without exposing provider mechanics.
- Never silently overwrite a destination.
- Prefer a small local tool over background infrastructure.
- Optimize for trusted household use, not an adversarial environment.
- Keep layout and metadata policies replaceable without redesigning the
  workflow.
- Preserve source material in the separate groomed result wherever practical;
  polishing the album should not mean silently discarding useful accompanying
  files.

## Out of scope for the first version

- background polling, watchers, queues, or a daemon;
- a database or durable job system;
- whole-library scans or rewrites;
- direct Navidrome database integration;
- automatic publication into the live library;
- retained source archives or backup machinery;
- NixOS modules, services, timers, users, groups, or capabilities;
- lyrics acquisition or normalization.
