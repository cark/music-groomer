# Decision index

The product and technical questions raised during initial alignment were
resolved or explicitly deferred. Post-v0.1 alignment is tracked below.

Detailed decisions are grouped into small pages:

- [Workflow and layout](decisions/workflow-and-layout.md)
- [Source inspection](decisions/source-inspection.md)
- [Files, tags, and artwork](decisions/files-tags-artwork.md)
- [Standalone tracks and scope](decisions/standalone-tracks-and-scope.md)
- [Milestone 3b loose-track identification](decisions/milestone-3b-identification.md)
- [Technical boundaries](decisions/technical-boundaries.md)
- [Milestone 3a review corrections](decisions/milestone-3a-review.md)
- [Milestone 3a real-world polish](decisions/milestone-3a-real-world-polish.md)
- [Milestone 4 final review corrections](decisions/milestone-4-review.md)
- [Existing-release replacement](decisions/existing-release-replacement.md)

Implementation must not begin merely because this index has no open entry. The
user must explicitly confirm overall alignment, as required by `AGENTS.md`.

When a new unresolved question appears, add it briefly below and resolve it one
question at a time. Once settled, move the durable result to the relevant
decision page.

## Open questions

Milestone 4 and its final-review corrections are accepted. The first post-v0.1
milestone is aligned around whole-release replacement, while incremental
completion remains deferred.

Replacement follows a newly corrected canonical destination path when that
differs from the selected release path. Replacement uses the existing guided
command and `Apply` action, with prominent replacement state and a separate
confirmation defaulting to No; an external collision remains a refusal.

Retained versions live under a marked, Navidrome-excluded recovery directory
inside the library and are managed through `music-groomer recovery`. Stable
lineage metadata and an active receipt prevent identity guesses. Restore keeps
the displaced active version and revives the selected version's historical
path. Manual deletion removes one selected retained version at a time.

Milestone 5 will also evict old retained versions automatically, but each
new version first receives a protected grace period. That period defaults to 30
days and is user-configurable. A configurable total storage cap evicts the
oldest eligible versions, but protected versions may temporarily exceed it; a
later run rechecks without a daemon. The cap defaults to a configurable 10 GiB.
This milestone replaces complete release directories only; standalone-file
replacement remains an important follow-up. Recovery preferences live in the
existing user configuration and are visible and editable through
`music-groomer recovery`. Eligible automatic eviction does not prompt again and
reports each removal. It is available as scheduler-friendly
`music-groomer recovery maintain` and runs after every Apply confirmation,
before staging and free-space preflight. Restore swaps reset the newly retained
version's grace clock. Handled failures attempt rollback, while power loss and
hard-crash recovery remain a documented manual edge rather than durable
transaction machinery. Each retained version keeps the immutable protection
deadline calculated when it entered recovery; later preference changes affect
only versions retained afterward.

The automatic eviction mechanism is part of Milestone 5; only external cron or
systemd scheduling remains a later deployment integration.

No product question remains open. The user explicitly confirmed overall
Milestone 5 alignment and authorized implementation on 2026-08-28.
