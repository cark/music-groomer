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

Milestones 1, 2, and 3a are implemented. The command can currently inspect one
album directory or loose audio file, identify it through MusicBrainz, and
review source or Cover Art Archive artwork without modifying the source:

```text
nix develop -c cargo run -- SOURCE
```

The guided review reads supported audio tags and properties, inventories
ancillary files and artwork, reports warnings or blockers, uses a bounded
provider cache, and asks only when a match is genuinely ambiguous. Use
`--offline` before or after `SOURCE` to guarantee that no provider is contacted.
Destination access and Apply remain later milestones.

Run `music-groomer --help` for the primary workflow and global options, or
`music-groomer cache --help` for cache maintenance. The interface supports the
usual `-h`/`--help` and `-V`/`--version` forms.

Provider-cache status is read-only. Clearing shows the exact owned path and
requires confirmation:

```text
nix develop -c cargo run -- cache
nix develop -c cargo run -- cache clear
```

Smoke tests and other isolated runs can select an exact cache directory for the
whole invocation. Every cache operation uses the override, including status and
clearing:

```text
nix develop -c cargo run -- --cache-dir /tmp/music-groomer-smoke cache
nix develop -c cargo run -- --cache-dir /tmp/music-groomer-smoke cache clear
nix develop -c cargo run -- --cache-dir /tmp/music-groomer-smoke SOURCE
```

The caller owns the temporary directory's lifecycle. music-groomer marks its
cache and refuses to claim or clear a non-empty unmarked directory.

The cache defaults to 256 MiB. It can be changed in the platform user config
file with, for example, `cache_max_mib = 128`. Metadata is fresh for 30 days;
stale entries remain available as an explicit fallback and least-recently used
entries are pruned to keep the cache within its limit.

Normal tests never use the network. Maintainers can explicitly exercise the two
small live adapters without touching music or library paths:

```text
nix develop -c cargo test --test live_provider -- --ignored --nocapture
```

Agents and contributors should begin with
[agent-docs/00-start-here.md](agent-docs/00-start-here.md). The current execution
status is in [agent-docs/development-plan.md](agent-docs/development-plan.md).

## License

music-groomer is available under the [MIT License](LICENSE).
