# music-groomer agent guide

Immediately read [agent-docs/current-work.md](agent-docs/current-work.md) and
verify its claims against Git. It is the high-priority recovery checkpoint for
active multi-turn work and may be stale after interruption.

For any milestone alignment, implementation, exercise, review, correction, or
transition, read and follow
[agent-docs/milestone-workflow.md](agent-docs/milestone-workflow.md). This is
the canonical lifecycle and must be revisited at every milestone boundary.

Immediately before asking to accept or close a milestone, re-read that workflow
and verify that the terminal exercise, deliberate review, full verification,
and blocking corrections are complete. After explicit acceptance, triage the
flyby inbox before aligning the next milestone. Never collapse either boundary
into the acceptance request.

Start with [agent-docs/00-start-here.md](agent-docs/00-start-here.md).
Follow its task-based routing instead of loading every page by default.

Keep `current-work.md` at 30 short lines or fewer. Update it whenever the active
objective, authorization, completed tranche, remaining work, constraints, Git
state, or next action materially changes; refresh it before deliberate
compaction and before ending a long implementation turn. Replace obsolete text
rather than appending history. Temporary contents normally remain uncommitted,
must not be staged accidentally, and return to the tracked `No active work`
baseline when the task is handed off. Put rationale and durable decisions in
the routed wiki pages instead.

The files in `agent-docs/` are the durable product and engineering record for
this repository. Keep them short, link related pages, and distinguish accepted
decisions from proposals and open questions.

Do not begin application implementation until the user explicitly confirms
that the product workflow and the open decisions are aligned.

When alignment requires several product decisions, discuss exactly one concrete
question at a time. Explain the problem and a recommended answer, let the user
respond, record the result, and only then move to the next question. Do not
present a large questionnaire.

Do not access or modify real source music or the live Navidrome library until
the user provides the paths and explicitly approves that step. Automated tests
must use temporary fixtures.
