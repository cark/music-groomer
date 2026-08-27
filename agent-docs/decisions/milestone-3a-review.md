# Milestone 3a review corrections

The implemented provider, matching, cache, and guided-review milestone was
reviewed with the user one product point at a time. The following corrections
were individually accepted; application implementation awaits the user's final
overall confirmation.

## Provider discovery and fidelity

- Cap ordinary discovery at the first eight plausible release groups so
  MusicBrainz rate limiting cannot turn a normal lookup into dozens of detail
  requests.
- For each capped group, inspect official release variants for compatible
  structure. Collapse variants only when their groomed results are equivalent;
  do not arbitrarily keep the first exact release.
- Use release-group identity, title, artist credit, original year, and type for
  album-level metadata. Use a compatible official release for disc structure,
  track positions, track titles, track credits, and recording IDs. Preserve an
  existing exact release ID but neither assert nor rewrite one.
- Prefer release-track titles and artist credits, falling back to recording
  values only when track-level data is absent. Keep credited names and join
  phrases while retaining recording identities.
- Use release-group and recording IDs through MusicBrainz lookup/browse APIs,
  not incorrect search fields. A stale ID warns and falls back to textual
  discovery. Use coherent artist and album-artist IDs as strong evidence,
  including constituent collaboration identities.
- Represent EP explicitly and treat it as album-like for layout. Keep unusual
  primary types visible as Other and require a human choice rather than
  silently calling them albums.

## Matching and correction

- Automatic selection requires explicit evidence gates: complete unique
  mapping, credible album identity or identifiers, meaningful track evidence
  beyond position, and a clear lead. Additive score and album length cannot
  manufacture confidence.
- Map tracks by recording identity first, then unique title and compatible
  duration. Existing position is corroboration or a cautious fallback, not an
  authority that overrides better evidence.
- Prefer an official Single for a loose track only after the Single is a
  credible track match. A strongly identified album must beat an unrelated
  Single; ask when credible release choices remain.
- Preserve every materially distinct candidate in the session. Metadata review
  must explain the selected match, distinguish choices by disc/track structure
  and material track-list differences, show full candidate track details on
  request, and allow selection of another candidate or coherent existing tags
  without restarting.

## Provenance, warnings, and artwork

- Keep provider year and coherent source-year fallback separate through
  selection. A source fallback is applied only to the selected result, remains
  visibly unverified, and never counts as provider agreement.
- Record scoped provenance where resolved values mix origins: provider versus
  source metadata, source-year fallback, preserved identifiers, and source
  versus archive artwork. Defer arbitrary per-field selection because it would
  turn v0.1 into a tag editor and permit incoherent combinations.
- Consolidate and deduplicate source, metadata, cache, and artwork warnings in
  preview while retaining paths and concrete causes for review.
- Explicit provider refresh also checks Cover Art Archive. Failure keeps the
  cached image; changed artwork is visible before replacing the current
  selection.
- Cache a confirmed absence of archive artwork for 30 days. Explicit refresh
  bypasses it, transient failure never becomes a negative result, and cache
  status distinguishes images from confirmed absences.
- When coherent existing metadata carries one common release-group ID, archive
  artwork may be offered with unverified source-ID provenance. Prefer a source
  cover; when none exists, require confirmation before making that archive
  image canonical.

## Interaction boundary

Keep v0.1 line-oriented: one stable preview and a shallow action menu. `Review`
prints source files/tags, metadata alternatives, or consolidated warnings and
returns. Metadata is the only review section that changes a decision. Do not
grow a full-screen TUI.

A later graphical interface is plausible because artwork comparison and audio
preview provide real value. If a second frontend is demonstrated, consider a
Rust workspace with a core crate, the current CLI crate, and separate advanced
UI crates. Do not split the workspace or add a web server in v0.1 merely to
prepare for that possibility.
