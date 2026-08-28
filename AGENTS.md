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

## Subagent delegation

Delegation is optional. The root agent may run zero to three subagents
concurrently only when the work divides cleanly into independent, bounded
streams. Subagents must not delegate recursively.

Use Luna at medium effort for bounded exploration, focused review, and
mechanical work. Escalate a difficult bounded investigation, implementation,
or review to Luna at xhigh only when additional reasoning is likely to improve
the result materially. Use Sol at medium only for bounded advisory work on
architecture, data safety, broad final audits, or conflicting findings. Sol
does not own product or safety decisions; the root agent synthesizes the
evidence and discusses consequential choices with the user. Treat these as
routing defaults when the named models are available, not as quotas.

Every subagent brief must be self-contained: state the objective, relevant
repository context, accepted constraints, current authorization, exact scope,
required evidence, stopping conditions, and expected report. Subagents are
read-only unless the brief explicitly authorizes writes after milestone
implementation authorization. For writes, assign disjoint files or
directories. Reserve shared contracts, `agent-docs/current-work.md`, and all
staging, committing, branch, milestone, acceptance, and push operations for the
root. A subagent must not edit outside its ownership, run broad source-rewriting
commands, revert others' work, access real music or the live library, or guess
through a behavior, safety, contract, or ownership ambiguity; it must stop and
report the issue instead.

The root owns product dialogue, shared contracts, task decomposition,
integration, synthesis, and final verification. It must inspect every delegated
change and run the relevant integrated verification itself. Delegation does not
replace any step in the milestone workflow, including alignment and explicit
implementation authorization, the user's terminal exercise, deliberate review,
blocking corrections, explicit acceptance, post-acceptance flyby triage, or
fresh permission immediately before every push.
