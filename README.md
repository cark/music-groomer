# music-groomer

A standalone Rust tool for carefully grooming album and loose-track metadata,
artwork, filenames, and library layout without modifying the selected source.

> [!WARNING]
> **Pre-alpha:** music-groomer is under active design and is not ready to groom
> or publish a real music library yet. There are no compatibility or support
> promises at this stage.

## Why this exists

music-groomer is an opinionated alternative for people who want a smaller,
safer ingestion workflow rather than a general-purpose tag editor or another
music-library manager:

- process one explicitly selected album or loose track;
- make the ordinary path guided and mostly automatic;
- leave the selected source untouched;
- show an exact preview before publishing anything;
- build and validate a separate result before it reaches the library;
- preserve embedded artwork and useful ancillary files;
- produce predictable, Navidrome-friendly tags, layout, and sidecar artwork;
- require no database, daemon, watcher, or plugin stack.

The project is primarily being built for one household workflow, but is public
because that narrow workflow may also suit other Navidrome and self-hosted music
users. Public availability must not inflate the deliberately small scope.

Milestones 1 and 2 are implemented. The command can currently inspect one album
directory or loose audio file without modifying it:

```text
nix develop -c cargo run -- SOURCE
```

The guided review reads supported audio tags and properties, inventories
ancillary files and artwork, and reports warnings or blockers. Provider
matching, destination access, and Apply are later milestones.

Agents and contributors should begin with
[agent-docs/00-start-here.md](agent-docs/00-start-here.md). The current execution
status is in [agent-docs/development-plan.md](agent-docs/development-plan.md).
