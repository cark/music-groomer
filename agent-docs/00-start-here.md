# Start here

music-groomer is a standalone, explicitly invoked tool that turns one selected
source album directory or loose track into a separate, library-ready result. It
must never modify the selected source.

## Current phase

Product and technical direction are aligned through milestone 2. Milestone 1
and milestone 2 are implemented, verified, reviewed, and accepted. Milestone 3
is split into 3a and 3b. Milestone 3a is aligned but awaits explicit
implementation authorization; milestone 3b remains pending.

## Task routing

Read [development plan](development-plan.md) for current status and the active
milestone. Then load only the pages relevant to the task:

- Product direction or scope: [product intent](product-intent.md) and the
  [decision index](open-decisions.md).
- Guided interaction or layout: [user workflow](user-workflow.md) and
  [workflow and layout](decisions/workflow-and-layout.md).
- Source inspection or format work:
  [source inspection](decisions/source-inspection.md) and
  [files, tags, and artwork](decisions/files-tags-artwork.md).
- Metadata semantics: [metadata policy](metadata-policy.md), plus the relevant
  decision page linked from the decision index.
- Architecture, providers, cache, or Apply: [technical shape](technical-shape.md)
  and [technical boundaries](decisions/technical-boundaries.md).
- Standalone tracks or deferred features:
  [standalone tracks and scope](decisions/standalone-tracks-and-scope.md).

Before changing accepted behavior, check the decision index for an existing
decision or deferral. Do not load unrelated pages merely because they exist.

## Documentation conventions

- Keep each page focused and manageable.
- Record why a decision exists, not just its outcome.
- Label unsettled ideas as proposals rather than silently turning them into
  requirements.
- Link to the relevant page instead of duplicating detailed policy.
- Add a TODO when history or rationale is missing; do not invent it.

## Alignment style

The user prefers deliberate, conversational alignment: raise one concrete
product question at a time, explain the underlying problem and recommendation,
wait for the answer, and record it before moving on. Do not bundle open choices
into a questionnaire. Implementation begins only after the user explicitly
confirms overall alignment.
