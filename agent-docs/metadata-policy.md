# Metadata policy

## Accepted artist behavior

Artist credit is not the same thing as a compilation flag.

For a collaboration album, preserve the credited collaborators as the album
artists. For example, a duo album credited to Niels-Henning Ørsted Pedersen
(NHOP) and Kenny Drew should expose both people:

- a natural display credit containing both names;
- separate artist values when the tag format and Navidrome support them;
- both corresponding artist identifiers when known.

Do not reduce a collaboration to the first artist and do not label it `Various
Artists` merely because more than one person is credited.

Use `Various Artists` for a true compilation whose tracks are credited across
otherwise separate artists and whose album-level credit is various artists.
Track artist credits still remain specific to each track.

Write compilation status explicitly: set it for such a true compilation and
clear an incorrect source flag for an ordinary or collaboration release.

Use the credited name from the selected metadata rather than inventing a fixed
joiner such as `&`, `and`, or `with`.

## Accepted tag preservation

- Modify only fields covered by the reviewed plan.
- Preserve existing embedded artwork.
- Do not add newly downloaded artwork to audio files.
- Do not remove embedded artwork for normalization.
- Preserve unrelated useful tags unless a later policy explicitly says
  otherwise.
- Do not fetch or normalize lyrics.
- Preserve existing genre and ReplayGain tags without fetching, inferring,
  splitting, normalizing, or replacing them. Leave genre absent when the source
  has none.

The groomed directory should also preserve accompanying source files wherever
practical. Any exception or transformation must be visible in the preview.

The preview should warn when preserved embedded images disagree with the chosen
album sidecar, because some clients may display the embedded image for an
individual track.

## Accepted artwork selection policy

Do not resize, upscale, recompress, or reject existing source artwork based on
dimensions. Preserve all source artwork with the other ancillary material.

When a recognizable source front cover exists, use it as the canonical cover
by default regardless of size. Offer the Cover Art Archive front as a visible
alternative. When no source cover exists, use the 1200-pixel Cover Art Archive
front rather than downloading its potentially much larger original scan.

Do not invent a visual-quality score in v0.1: dimensions, byte size, and format
do not establish that an image is attractive or correct. Any replacement must
be visible in preview. Do not download booklets, backs, or scan collections.

If the user selects a replacement for an existing root-level cover, put the
selected image at the album root as `cover.<native extension>`. Preserve
displaced source cover files byte-for-byte under `original-artwork/` so
Navidrome cannot accidentally prefer them. Show every relocation in preview.

## Accepted date behavior

Use the album's original release year as its canonical `DATE` and as the year
used by the destination layout. A later reissue or edition may supply the
matching track list without changing the album's displayed year.

If a confident provider match lacks an original release year, preserve an
existing source year and mark it as unverified. If neither provider nor source
supplies a year, leave it absent and warn rather than inventing one.

For a compilation, this means the compilation album's own original release
year, not the original release years of its individual songs.

## Accepted identifier behavior

Write MusicBrainz identifiers for artists, album artists, confidently mapped
recordings, and the release group when confidently known. A missing replacement
identifier means preserve the existing value rather than delete it. v0.1 does
not add or change a specific release ID, even if a representative edition
supplies metadata; preserve an existing release ID unchanged. Show identifiers
only in expanded preview details during the ordinary guided workflow.

## Accepted standalone-track behavior

When a loose track is confidently identified, associate it with a real release
by default so its album metadata and artwork are genuine. Present a small
human-readable choice when materially different releases remain plausible. If
no release is defensible, leave the track albumless rather than creating a fake
one-track album.

Treat the fact that the source contains one loose track as evidence that it is a
single. Prefer a matching official single release. Only fall back to a studio
album release when no matching single exists, unless credible existing metadata
or identifiers say otherwise.

A matched single receives its own release directory and canonical cover using
the same general layout policy as an album. Do not collect unrelated singles in
one artwork-sharing directory.

An unmatched loose track remains albumless and is placed under the artist's
`Standalone Tracks/<title>/` directory. Do not invent an album, year, or track
number to make it resemble a release.
