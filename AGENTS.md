# music-groomer agent guide

Start with [agent-docs/00-start-here.md](agent-docs/00-start-here.md).
Follow its task-based routing instead of loading every page by default.

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
