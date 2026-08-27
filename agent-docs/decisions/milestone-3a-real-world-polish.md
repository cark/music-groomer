# Milestone 3a real-world polish

The first authorized read-only Evolution exercise exposed two real-source bugs
and inconsistent styling across later guided screens. The user confirmed the
complete correction set, which was implemented and verified on 2026-08-27.

## Semantic terminal presentation

- Application-owned human output uses a small semantic vocabulary for
  headings, fields and values, paths, statuses, menu items, prompts, selected
  choices, and alternatives.
- Ordinary explanatory sentences use `prose`, deliberately named to discourage
  using the escape hatch for structured output.
- Core and provider logic continue returning structured domain values and
  events. Terminal presentation types remain at the human CLI boundary; a
  future machine interface consumes the domain values directly.
- Direct printing is mechanically forbidden outside the terminal renderer,
  Clap-controlled output, and a last-resort renderer-failure diagnostic.
- The centralized initial palette is bold headings and values, subdued/bold
  field names, cyan paths and menu keys, green success, yellow warnings, red
  errors, and normal prose. It remains easy to revise, terminal-aware, honors
  `NO_COLOR`, and never carries meaning without text.
- Tests primarily assert semantic output, with focused colored/plain renderer
  tests and a few readable end-to-end transcripts rather than exhaustive ANSI
  snapshots.
- Migrate every application-owned CLI surface in one focused pass: inspection,
  guided matching and artwork, provider progress, cache commands, internal
  demo, and fatal diagnostics.

## Real-source corrections

- Continue content-probing every ordinary file so valid disguised audio still
  works. A parse failure blocks as corrupt audio only for an audio-like
  filename; otherwise image and ancillary inspection continues. Genuine read
  failures still block.
- MusicBrainz discovery first tries the coherent source album title unchanged.
  If no usable group is found, visibly retry with a search-only base title made
  by removing trailing edition-like parentheses or brackets. Do not change the
  source value during search, and cache the complete outcome under the original
  query.
- The accepted sole-candidate correction remains: one credible candidate
  auto-selects and stays revisable; one uncertain candidate uses Yes/No rather
  than a numbered list containing one item.

## Scope and validation

This pass includes regression tests, documentation, cache-schema invalidation
where provider semantics changed, and a read-only rerun against the authorized
Evolution directory. It does not add layout work, Apply, fingerprinting, a
workspace split, or wider matching redesign.

## Implementation record

- Human CLI output now crosses one semantic `Interaction` boundary. The stdio
  renderer owns ANSI styling, while fakes retain semantic lines and readable
  plain transcripts.
- Clippy denies direct standard print macros across both application crates;
  Clap output, the renderer's direct `Write` calls, and the final renderer-error
  fallback remain the narrow exceptions.
- Non-audio parse failures fall through to artwork and ancillary inspection,
  while audio-like corrupt files and genuine read failures remain blockers.
- MusicBrainz emits a structured visible retry before using a search-only base
  title, only after the exact title produces no usable candidate. Cache schema
  3 prevents old literal-title misses from bypassing the new behavior.
- Offline tests cover semantic menu/path events, colored and plain rendering,
  common real-source ancillary types, exact-title precedence, and the fallback
  retry. The final Evolution rerun remains a user-driven read-only demo in the
  user's own terminal.
