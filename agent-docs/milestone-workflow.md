# Milestone workflow

This is the canonical lifecycle for every milestone. Re-read it at milestone
boundaries; keep milestone-specific requirements in the development plan.

1. After the previous milestone is explicitly accepted, triage the
   [flyby inbox](flybys/README.md) before aligning new work. Triage does not
   authorize fixes or reopen the accepted milestone.
2. Align consequential product choices one question at a time. Use a decision
   pulse of at most three short items for related reversible details.
3. Summarize the aligned milestone and obtain explicit implementation
   authorization. Activate the compact `current-work.md` checkpoint.
4. Implement in coherent tranches with small local commits. Record non-blocking
   unrelated discoveries as flybys instead of starting side quests.
5. Offer the user an early terminal exercise whenever a meaningful vertical
   slice exists. Clearly label behavior that is not implemented yet.
6. Run proportionate automated verification and a deliberate code review,
   including correctness, duplication, unnecessary infrastructure, and test
   value.
7. Fix blocking findings. Apply the stopping rule: defer attractive polish,
   rare edges, and structural work the next milestone will not worsen.
8. Present the acceptance evidence and ask explicit permission before recording
   the milestone as accepted or complete.
9. Reset `current-work.md` to `No active work`, finish coherent local commits,
   and ask separately for fresh permission immediately before every push.

If evidence contradicts an accepted decision, stop and realign. A correctness,
safety, or current-milestone blocker is raised immediately rather than filed as
a flyby.
