# Start here

music-groomer is a standalone, explicitly invoked tool that turns one selected
source album directory or loose track into a separate, library-ready result. It
must never modify the selected source.

## Current phase

Product and technical direction is aligned through milestone 3b. Milestones 1,
2, 3a, and 3b are implemented, reviewed, verified, and accepted by the user.
The final milestone 3b loose-library-track exercise confirmed the corrected
single discovery, animated provider feedback, and phase spacing on 2026-08-28.
The accepted post-milestone-3 review corrections are implemented and verified
locally; they have not been pushed.
Milestone 4 alignment has not begun.

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
- Current post-review polish:
  [Milestone 3a real-world polish](decisions/milestone-3a-real-world-polish.md).
- Standalone tracks or deferred features:
  [standalone tracks and scope](decisions/standalone-tracks-and-scope.md).
- Milestone 3b fingerprinting and AcoustID behavior:
  [loose-track identification](decisions/milestone-3b-identification.md).

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

Positive feedback on an exercise or correction does not by itself close a
milestone. Summarize the acceptance evidence and ask for explicit permission
before recording a milestone as accepted or complete. Likewise, ask for fresh
explicit permission immediately before every push; do not treat an older or
conditional push discussion as standing authorization.

Treat alignment as collaboration rather than approval seeking. Give a genuine
recommendation and raise substantive pushback when evidence, tradeoffs,
contradictions, or a simpler design warrant it. Do not manufacture objections
merely to demonstrate independence or to satisfy this rule; agreement is the
right answer when the proposal is sound.

Keep asking one question at a time for choices that affect visible behavior,
data meaning, safety, complexity, or future direction. Reversible internal
details that follow accepted principles do not each require approval, but they
should not become invisible: after a meaningful discussion step, a short
"decision pulse" may group at most three related minor choices in one line with
the intended defaults. The user can say to continue or stop on any item that
sparks an idea. Do not turn the pulse into a questionnaire or a wall of text.

Before a large implementation boundary after lengthy alignment, proactively
suggest a conversation compaction and provide the exact resume phrase. This
keeps the handoff cheap without making the user spend a turn asking whether the
durable documentation is sufficient.
