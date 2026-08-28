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
selected by the user. The normal output is a new groomed album or track under
the configured Navidrome media-library root. The source remains untouched.

Standalone tracks are part of v0.1 because the user's main listening playlist
contains many loose tracks and their artist, title, artwork, and presentation in
car clients matter. This does not authorize scanning or rewriting the playlist
or complete library.

Keep acoustic identification narrow: it is an optional fallback for one
selected loose track, not a bulk fingerprinting pipeline or submission service.

The project may be published for other Navidrome and self-hosted music users,
but remains primarily shaped by the demonstrated household workflow. Describe
it as experimental or pre-alpha until safe Apply and the real-album exercise
succeed. Public availability creates no compatibility or support promise and
must not broaden v0.1 speculatively.

Process one explicitly selected item per guided v0.1 session: either one album
directory or one loose track. Playlist and multi-item batch processing are
deliberately deferred until the single-item workflow is proven.

v0.1 always creates a new destination and refuses a collision. Updating or
replacing an explicitly selected album or track already inside the live library
is the next distinct destination workflow; do not mix replacement safety into
v0.1.

That deferred workflow must include completing or rebuilding a release from
tracks encountered separately. This is a meaningful product gap, not optional
polish: preserve accurate release positions now and make an existing-release
collision understandable, then design safe incremental completion after v0.1.

The first post-v0.1 workflow is now aligned around replacing one explicitly
selected complete release directory. Incremental completion and replacement of
an explicitly selected standalone file remain important later workflows.

Selecting one audio file selects only that file; arbitrary siblings do not
become source material. Selecting a directory recursively selects one logical
release together with its ordinary ancillary contents. A directory containing
one audio file is therefore a single-release source rather than an implicit
batch.

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
- updating or replacing an existing live-library item;
- retained source archives or backup machinery;
- NixOS modules, services, timers, users, groups, or capabilities;
- lyrics acquisition or normalization.
