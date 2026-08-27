# music-groomer

A standalone Rust tool for carefully grooming album and loose-track metadata,
artwork, filenames, and library layout without modifying the selected source.

Milestones 1 and 2 are implemented. The real command can now inspect one album
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
