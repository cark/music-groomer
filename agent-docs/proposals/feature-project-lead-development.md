# Feature and project-lead development experiment

Status: tentative future plan. This does not yet replace the current milestone
workflow or authorize implementation, worktree creation, merging, or pushing.

After the accepted Milestone 5 work is handed off, future development is
intended to move from numbered milestones to independently aligned and accepted
features. Historical milestone records remain unchanged.

## Units of work

- A **feature** is a coherent product outcome that can be aligned, exercised,
  reviewed, and accepted independently.
- A **work item** is one small implementation or correction commit with a
  frozen contract and focused verification.
- A **project** is a temporary engineering effort containing one or more
  related features and their work items.
- A **project lead** is a context-bearing engineering lead that exists only as
  long as its project benefits from continuity.

Feature boundaries do not make dependencies disappear. Product discussion may
continue while authorized work runs, but implementation may proceed in parallel
only when the project lead has established that the work items are independent.

## Roles

- The root agent remains the user's product partner: it owns product dialogue,
  consequential decisions, implementation authorization, terminal exercises,
  feature acceptance, and fresh push permission.
- A Sol-medium project lead performs technical preflight, challenges the
  design, decomposes authorized features, owns integration and full technical
  verification, and reports unresolved risks. It does not ordinarily write
  feature code or silently decide product, safety, or data-meaning questions.
- Luna-medium developers implement small, bounded work items in isolated Git
  worktrees and deliver one or more explicitly assigned candidate commits.
- Luna-xhigh reviewers provide independent adversarial review where the risk or
  difficulty justifies it. Sol-medium review remains available for architecture
  and data-safety grilling.
- The root agent may act as the mechanical scheduler while the project lead is
  dormant; scheduling does not transfer engineering ownership back to the root.

Completed agents may remain dormant for later follow-up without occupying an
active slot. A developer can therefore be resumed with independent review
findings, and the project lead can be woken only for meaningful design and
integration boundaries.

## Candidate-commit pipeline

1. The user and root align a feature one consequential question at a time.
2. The project lead performs technical preflight and returns missed
   consequential questions to the user through the root.
3. The root summarizes the aligned feature and obtains explicit implementation
   authorization.
4. The project lead defines dependency-aware work items with exact scope,
   ownership, acceptance evidence, and stopping conditions.
5. Luna developers work on unique branches in isolated worktrees, run focused
   checks, and report base and candidate commit IDs, changed files, verification,
   limitations, and deviations. No GitHub pull request is required.
6. An independent reviewer examines the commit and contract. Blocking findings
   return to the original developer when retaining its context is useful.
7. The project lead integrates approved commits, delegates semantic conflict
   corrections instead of guessing through them, and runs the integrated gate.
8. The root guides a meaningful user exercise and presents acceptance evidence.
9. The user explicitly accepts, defers, or rejects the feature. Push permission
   remains a separate fresh decision.

The root does not idle-poll workers. It continues useful product dialogue and
receives or checks completion events at natural boundaries. A genuine
dependency or acceptance gate still waits for its required evidence.

## Continuity and safeguards

The project lead is deliberately temporary. Durable decisions, accepted
behavior, candidate commits, verification, and handoff state belong in the
repository rather than in one agent's transcript. A project ends with a concise
handoff and clean worktree/integration state; a later project may use a fresh
lead.

The current safety and collaboration principles remain intended defaults:

- no application implementation before explicit feature authorization;
- no real source music or live-library access without path-specific approval;
- temporary fixtures for automated tests;
- prominent reporting of evidence that contradicts accepted behavior;
- independent review, blocking corrections, and proportionate verification;
- early user exercises for meaningful vertical slices;
- focused local commits and fresh permission immediately before every push;
- unrelated discoveries go to the flyby inbox rather than becoming side quests.

## Open setup details

Before activating this experiment, align and record:

- the permanent or per-project worktree root;
- integration-branch and canonical-`main` ownership;
- the exact candidate-commit and review report formats;
- concurrency, retry, correction-cycle, and stop limits;
- lightweight measures for elapsed time, unique findings, rework, conflicts,
  duplicated verification, and the user's observable subscription consumption.
