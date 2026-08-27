# Milestone 3b loose-track identification

This page records the narrow acoustic-identification fallback for one selected
standalone track. General standalone behavior and deferred scope remain in
[Standalone tracks and scope](standalone-tracks-and-scope.md).

## Identification fallback

For one selected loose track, optionally use `fpcalc` and AcoustID only when
identifiers, tags, filename, and duration are insufficient. Cache the lookup,
degrade gracefully when unavailable, never submit fingerprints, and do not
routinely fingerprint album tracks.

Run this fallback automatically within the same guided interaction rather than
requiring a separate command or confirmation. Show fingerprint calculation,
AcoustID access, waits, retries, cache use, and failures with the same visible
progress conventions used for MusicBrainz. If the optional tooling or provider
is unavailable, explain the cause and continue to the existing-metadata or
unmatched-track outcome.

Ordinary progress must state that AcoustID receives the compact fingerprint and
duration, not the audio file. Expanded evidence records the provider request
provenance. This transparency does not add another confirmation prompt to the
already automatic fallback.

Treat an AcoustID result as evidence for one or more MusicBrainz recordings,
not as authority for an album or single. Resolve those recording candidates
through the existing MusicBrainz release-selection policy: prefer an official
single for a lone track when appropriate, retain materially different release
choices, and never select a release solely from an AcoustID score.

Trigger fingerprinting when the recording identity itself is missing or
uncertain, including when ordinary lookup leaves plausible but ambiguous song
candidates. Do not fingerprint merely to choose among releases after the
recording identity is already credible: the same recording may legitimately
appear on a single, album, or compilation, so fingerprinting cannot settle that
release-level choice.

Ship the project's registered AcoustID application key with music-groomer. It
identifies the application and is not the user credential required for
submissions. A distributed open-source client cannot keep this value secret, so
treat it as publicly observable rather than pretending to conceal it. Confirm
that the registration terms permit embedding before committing the key; stop
and realign if they unexpectedly require confidentiality. Do not require users
to create an AcoustID account or configure a personal key; music-groomer remains
lookup-only and never submits fingerprints.

The user registered music-groomer with AcoustID on 2026-08-27. The registration
flow showed no confidentiality restriction, and the project application key is
available for the authorized implementation. Do not record the user's account
email in this repository.

Calculate the compact fingerprint whenever this fallback is needed and use the
fingerprint plus duration as the AcoustID cache identity. Do not hash the whole
source file or maintain a separate persistent fingerprint layer merely to avoid
this cheap local calculation. Store only the AcoustID response and cache
metadata; never retain audio samples or a copy of the track. These entries share
the existing bounded provider cache.

Treat the AcoustID association as provider metadata with the existing 30-day
freshness period: every fallback calculation determines the cache key, a fresh
hit then bypasses AcoustID, a stale hit refreshes the provider result, and a
failed refresh may visibly fall back to the stale result. Explicit refresh
bypasses provider freshness but still uses the newly calculated fingerprint.

Cache a successful AcoustID no-match response for the same 30-day period so
repeated previews do not immediately repeat a fruitless lookup. Keep this
distinct from provider errors: timeouts, malformed responses, and other
failures never become negative entries. Explicit refresh retries a cached
no-match immediately.

Give MusicBrainz and AcoustID separate cumulative 30-second transient-failure
recovery budgets for one identification run. This avoids allowing the first
provider to consume the second provider's opportunity while bounding combined
failure-related waiting to roughly 60 seconds. Successful requests and
MusicBrainz's mandatory one-request-per-second spacing are not failures and do
not consume this retry budget. Use increasing retry delays, keep all requests
within provider policy, show which provider is waiting, and retain ordinary
immediate `Ctrl-C` termination.

Use `fpcalc`'s standard fingerprint over at most the first 120 seconds of audio.
Run it once with visible progress and a hard 60-second wall-clock timeout. On
timeout or process failure, terminate the child, explain the cause, and continue
without fingerprint evidence. Do not retry the same local calculation
automatically because an immediate repeat is unlikely to change the outcome.

Use conservative named score gates rather than treating AcoustID's score as a
probability. Ignore results below `0.80`. Results at or above `0.80` may add a
MusicBrainz recording candidate. Fingerprint evidence may participate in
automatic acceptance only at `0.90` or above, after all qualifying AcoustID
results collapse to one MusicBrainz recording, duration is compatible, and no
credible existing identifier or tag contradicts it. Otherwise retain an
understandable user choice. Keep these constants fixed in v0.1 and revisit them
only if real smoke-test evidence warrants it.

In the normal human view, describe fingerprint provenance in plain language,
such as "Audio fingerprint supports this recording" or "Audio fingerprint was
ambiguous"; do not present the raw score as a percentage probability. Expanded
details may show the raw score, AcoustID identifier, and MusicBrainz recording
identifier. Preserve all scores and provenance as structured workflow data for
a future machine-facing interface.

An AcoustID result without a MusicBrainz recording association is not usable
metadata. Report that AcoustID recognized the audio but lacks a MusicBrainz
association, cache the result normally, and continue to the existing-metadata
or unmatched-track outcome. Do not turn it into guessed artist/title data or
add fuzzy manual searching in this milestone.

Extend the existing cache interface rather than adding fingerprint-specific
commands. Status reports fresh, stale, and cached-no-match AcoustID counts and
size. Cache-directory override, bounded pruning, obsolete/damaged reporting,
and confirmed clearing apply coherently to these entries.

In offline mode, calculate the fingerprint locally to look for a cached
identification. Use cached AcoustID data even when stale, with clear status. If
no cached identification exists, explain that provider identification is
unavailable offline; no network request occurs and the calculation itself
creates no cache entry.

Enable the fallback whenever inspection classifies the selected content as one
genuine standalone track, whether the user selected the audio file directly or
a directory containing that track and ancillary material. Do not fingerprint a
one-track source that already has credible album or single identity merely
because it contains one audio file.

Use the mature reference `fpcalc` executable behind a narrow child-process
boundary rather than adding a second in-process audio-decoding stack for v0.1.
The repository's Nix development shell should provide it, and future official
packages should make it available automatically rather than requiring a manual
user installation. A raw executable that cannot find `fpcalc` reports the
optional fallback as unavailable and continues normally.

Keep `fpcalc` as a separate packaged helper. Chromaprint's own code is MIT, but
official static `fpcalc` builds include LGPL FFmpeg components; any later binary
bundle must ship the applicable notices and satisfy corresponding-source and
relinking obligations for the exact artifact. This does not change
music-groomer's MIT license. Reconsider a pure-Rust Chromaprint and decoder path
only when it is mature across every accepted audio format or external-helper
distribution demonstrates a real problem.

Do not add an AcoustID identifier or full fingerprint to groomed audio tags in
v0.1. AcoustID is evidence used to obtain a confident MusicBrainz recording ID,
which is already part of the accepted identifier policy. Preserve any existing
AcoustID-related tags as unrelated source data.

Do not add format-specific matching from pre-existing AcoustID tags in this
milestone. Preserve them, but calculate a fresh fingerprint from the actual
audio whenever the fallback is needed; the measured local cost is much smaller
than the extra lookup, cache, and conflict paths would justify.

Always identify local fingerprint calculation explicitly in progress output so
its delay cannot be mistaken for a hang or network access. Milestone 3b performs
this once for one standalone track, never routinely across an album. If album
fingerprinting is introduced later, it must expose per-track and accumulated
progress rather than silently multiplying the work.

Bound provider fan-out after fingerprinting. Discard scores below `0.80`,
collapse duplicate associations to the same MusicBrainz recording, and resolve
at most the five highest-scoring distinct recordings through MusicBrainz. Keep
their evidence in structured workflow data. If more than five qualifying
recordings existed, visibly warn that the fingerprint was unusually ambiguous
rather than implying exhaustive certainty. This bound protects provider
capacity as well as the guided interaction.

Request only MusicBrainz recording identifiers from AcoustID, plus the result
identifier and score inherent in its response. Do not request AcoustID's
expanded release, release-group, or track metadata. Resolve only the bounded
recording candidates through MusicBrainz under its existing cache and rate
limiter, keeping metadata authority and response size narrow.

Send the fingerprint and duration in an HTTPS POST body rather than placing the
long fingerprint in a query URL where intermediaries may log it. This changes
neither the lookup-only policy nor the structured provider boundary.

MusicBrainz WS/2 has no canonical batch lookup for arbitrary recording MBIDs,
and a recording lookup embeds at most 25 linked releases in arbitrary MBID
order. Do not misuse indexed Lucene search as a pseudo-batch endpoint or rely on
that truncated embedded list. For each distinct recording, browse one cached
page of up to 100 official single releases first. Only when that yields no usable
single, browse one cached page of up to 100 official album releases. Collapse
releases into materially distinct groomed results and stop at those bounded
pages rather than walking a popular recording's complete appearance history.
Pace every request through the existing one-request-per-second limiter. With at
most five recording candidates, this discovery phase makes at most ten
MusicBrainz browse requests before the existing bounded variant resolution.
