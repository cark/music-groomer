# Existing-release replacement

## Accepted milestone scope

The first post-v0.1 milestone will support safely re-grooming one explicitly
selected existing release as a complete replacement. It will construct and
validate the whole proposed release before changing the existing library item.

This milestone will not merge individual files into an existing release or
incrementally complete a release from separately encountered tracks. That
remains the following distinct destination workflow because it adds track-level
identity, compatibility, collision, and interruption questions.

When corrected metadata changes the canonical destination directory, the
replacement follows that corrected path. Relocation is part of replacing the
release rather than a reason to retain a knowingly noncanonical path or refuse
the useful correction.

After successful replacement, retain the complete old release as a recoverable
copy. Validation makes publication safe, but it does not eliminate later human
regret or a semantically wrong choice. Report the retained recovery copy
clearly. A retained version remains available through an aligned protected
period, after which Milestone 5 may evict it automatically under the
bounded policy below.

Store retained originals under a dedicated `.music-groomer-recovery` directory
inside the configured library root. This keeps recovery on the library's own
mount and avoids requiring a writable parent, sibling-path convention, or
separate recovery-root configuration. Put a blank `.ndignore` in the recovery
directory so Navidrome excludes the complete subtree; the dot-prefixed name is
only additional protection. Mark the directory as music-groomer-owned and
verify its marker and `.ndignore` before every replacement. Never claim or
clean an unmarked directory. Report the exact retained-release path after a
successful replacement.

Manage retained releases through a dedicated `music-groomer recovery` guided
interaction rather than requiring manual browsing of the hidden directory or
adding recovery actions to the ordinary grooming menu. The replacement
completion screen reports the retained entry and points to that command. The
recovery interaction lists enough human-readable provenance to identify each
entry before any restore or explicit removal.

Create a stable tool-generated lineage identifier when a release first enters
replacement. Keep its version history and expected active path in owned
recovery metadata, and put a small hidden music-groomer receipt in the active
release containing the same lineage and active-version identifiers. Reuse that
lineage across later replacements and restores. Paths, names, and provider
identifiers are descriptive evidence, never recovery identity by themselves.
Before a restore, require the selected recovery lineage, expected active path,
and active receipt to agree. A missing or conflicting receipt or manifest stops
without moving or deleting anything.

Restore is a reversible swap within one lineage. Before activating the selected
retained version, move the currently active version into that lineage's version
history. Do not delete or overwrite the displaced active version. Update the
lineage metadata and active receipt only as part of the successful swap.
Treat the displaced active version as newly retained at the time of every swap:
reset its retention timestamp and protected-until clock using the effective
grace preference. The version being restored becomes active and has no running
recovery-eviction clock.

Restore the selected version to the exact library path it occupied when it was
retained, so content, metadata, and layout return as one historical state. Check
that path before moving either version. If unrelated content now occupies it,
refuse the restore without changing the active release or recovery history.

Manual cleanup removes one explicitly selected retained version at a time after
showing its historical path, retention date, and size and confirming with a safe
default. The active version is never eligible. Do not add bulk or whole-lineage
manual deletion. Separately, Milestone 5 will automatically evict old
retained versions under an aligned bounded policy.

Every retained version has a guaranteed grace period during which automatic
eviction is forbidden. Manual removal remains available during that period.
After the grace period, the version becomes eligible for the automatic policy;
eligibility does not require immediate deletion.

The grace period defaults to 30 days and is a user-configurable preference.
Different users may reasonably trade recovery time against storage differently;
do not hard-code the default as an unchangeable product rule.
Calculate and store an immutable protected-until time whenever a version enters
recovery. Later preference changes do not shorten or extend already retained
versions. A replacement or Restore swap that retains a version afterward uses
the then-effective preference for that new protected-until time.

Automatic eviction uses a user-configurable total recovery-storage cap and
oldest-eligible-first ordering. Treat the cap as soft while versions remain in
their grace period: a new retained version may exceed it, and protected versions
are never evicted merely to get back under the limit. Report the excess and the
earliest eligibility date. On a maintenance pass, re-evaluate and evict eligible
versions until the store is within its cap; if all candidates remain protected,
defer again without error. The core application needs no daemon; the explicit
maintenance command may be scheduled externally later.
Actual free-space preflight remains independent and may block a replacement
rather than evict a protected version.

The recovery-storage cap defaults to 10 GiB and is user-configurable. Protected
versions may still exceed that default as described above.

The recovery lifecycle and automatic eviction mechanism are part of this
replacement milestone. Only external cron or systemd scheduling of
`music-groomer recovery maintain` remains a later deployment integration.

This milestone replaces only an explicitly selected complete release directory;
a directory containing one track still qualifies. Defer an individual file
already inside the library because selecting one file deliberately owns only
that file, not its containing directory, artwork, or siblings. Standalone-file
replacement remains an important follow-up product feature, not optional polish.

Store the recovery grace period and storage cap in music-groomer's existing
user configuration alongside the destination-root preference. The dedicated
`music-groomer recovery` interaction displays the effective values and offers a
guided way to change them; do not require per-invocation recovery-policy flags.

Once an eligible version must be evicted under the configured cap, proceed
without another confirmation. Report every removed version with recognizable
release information, its retention date, and freed space, then show remaining
usage and the cap. If protected versions keep the store over its cap, report
that cleanup was deferred and why. The configured automatic policy is the
authorization; do not turn it back into per-run manual cleanup.

Expose the same deterministic maintenance pass as non-interactive
`music-groomer recovery maintain`, suitable for later cron or systemd scheduling
outside the core application. Also run it on every confirmed Apply, whether
publishing a new result or replacing an existing release. Preview, cancellation,
help, version, and unrelated maintenance commands do not trigger eviction.
For Apply, run maintenance immediately after its explicit confirmation and
before staging and free-space preflight. Its authorized eviction is independent
of whether the later Apply succeeds.

Keep interruption handling proportionate. Preflight the complete swap, validate
copies, order moves so complete versions are preserved, and attempt rollback for
handled errors in the current process. Do not add a durable transaction journal
or automatic restart recovery for power loss, reboot, or a hard crash. Document
that such an interruption may require manual filesystem recovery.

Use the existing `music-groomer SOURCE` guided workflow and its existing
`Apply` action rather than adding a replacement subcommand or a second main-menu
action. Selecting an existing release inside the configured library establishes
replacement context even when corrected metadata relocates it. Mark that state
prominently in preview, show a large replacement warning, and require a
replacement-specific confirmation immediately before Apply. An external source
that merely collides with an existing destination remains a refusal, not an
inferred replacement.

The replacement-specific confirmation defaults to No and proceeds only on an
explicit affirmative answer. Do not require a typed phrase or add confirmation
ceremony beyond the prominent warning and deliberate `y` response. Ordinary
new-result Apply retains its accepted confirmation behavior.

Product alignment is complete. This decision does not authorize application
implementation by itself.

## Alignment status

No product question remains open. The user explicitly confirmed overall
Milestone 5 alignment and authorized implementation on 2026-08-28.
