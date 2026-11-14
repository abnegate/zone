# Learning gap review: zone against claudear

What zone learns from its own past work, set against what `appwrite/claudear`
learns from its own, capability by capability, with what porting each missing
piece would take and which pieces are not worth porting.

Compared: claudear at `bc5fc75` (2026-09-09) and zone `main` at `334df85`
(2026-09-16). Both were inventoried file by file; the numbers below are the
constants in the code, not the ones in either README. Line references are to
those two commits.

## Contents

- [The two systems in a paragraph each](#the-two-systems-in-a-paragraph-each)
- [Capability by capability](#capability-by-capability)
- [What zone lacks, and what porting it would take](#what-zone-lacks-and-what-porting-it-would-take)
- [What zone has that claudear does not](#what-zone-has-that-claudear-does-not)
- [Zone's own broken links](#zones-own-broken-links)
- [What not to copy](#what-not-to-copy)
- [Suggested order](#suggested-order)
- [Since this review](#since-this-review)

## The two systems in a paragraph each

**Claudear** learns around an issue-fixing pipeline. Every attempt at an issue
records an outcome (`feedback_outcomes`, with a local embedding of the issue
text); a merged pull request triggers a post-merge pass that extracts
learnings from the agent's execution log, analyses the diff, accumulates
per-repository knowledge and scores the fix; each review event classifies the
reviewer's comments; a periodic pass (every tenth housekeeping tick, about
fifty minutes) promotes repeated Q&A answers to standing instructions, detects
issue clusters and cross-repository correlations. Six learned blocks are
prepended to every agent prompt, in a fixed order, from
`processing.rs:3228`. Storage is nine dedicated SQLite tables. Most bars are
low (a Q&A answer is promoted after two occurrences; a review pattern after
three comments) and about half the constants are hard-coded.

**Zone** learns around task runs and chats. A six-hourly learning pass
(`workers/learning/`) reads finished runs, their file changes and their tool
calls; it scores how each change was received (merge speed, review cycles,
approvals), classifies failures by embedding against exemplar sentences, and
from well-received runs learns repository conventions and strategy lessons,
each needing several distinct runs and a confidence above a bar before it is
written. A separate six-hourly pass clusters repeated chat questions by
embedding and promotes an agreed answer to a standing instruction, after
screening it through the same rulebook the memory tools use. Everything
learned is a `knowledge_entries` row with its provenance in tags, retirable,
and rendered into the task-run prompt as three blocks. Beside the loop sit a
regression watch, agent analytics, a scheduled digest and a static
prioritiser. Bars are high and every one is pinned by a test that was seen to
fail when the constant moved.

The short version: zone's loop is narrower but sounder. Claudear covers more
ground, and several of its capabilities are write-only or unwired. The gaps
worth closing in zone are mostly consumers and surfaces, not new learning.

## Capability by capability

| Claudear capability | Zone equivalent | Verdict |
|---|---|---|
| Outcome tracking per attempt, with an embedding of the issue (`feedback/outcomes.rs`) | `task_runs.artifacts` (`attempts`, `pr.*`, `review.comments`, `quality`, `failure`) folded by `learning::attempt` with a 14-day recency half-life | Comparable data. Zone does not embed the task itself, so it cannot find similar past runs. |
| "Learnings from similar issues" prompt block (top 5 by cosine, `min_sim 0.1`) | None | **Missing** (G1). |
| Similar-issue context and semantic duplicate skip at intake (`>= 0.90`) | None; zone has no issue intake | Mostly not applicable. A "similar open task" warning at creation is the only transferable half (G1, optional). |
| Semantic error categorisation, 8 categories, threshold 0.3 (implemented, never wired) | `learning::error_category`: 10 categories × 3 exemplars, max-cosine, similarity `>= 0.45` **and** margin `>= 0.05` over the runner-up, else `Unknown`; wired and consumed | Zone ahead. |
| Log extraction: root cause, files, strategy from the execution log, LLM first then regex | Strategy fingerprint from the structured tool-call record (`learning::strategy`); no root-cause text | Different method, zone's is more reliable; zone records no per-run "what was learned" text (G2, optional). |
| Diff analysis: file categories, extension histogram | `learning::observation`: naming, directory content, test placement and naming signals from paths and added lines | Zone ahead on what it infers; no browse surface (G6). |
| Q&A promotion: group by normalised answer, `>= 2` occurrences, optional first-pair cosine `>= 0.8` | `workers/promotion`: greedy embedding clustering (leader **and** centroid `>= 0.88`), `>= 4` occurrences across `>= 3` chats, answer agreement `>= 0.6`, screened for secrets, identifiers and suppressions | Zone ahead on soundness; claudear promotes far sooner. Worth a per-workspace switch if the six-hour cadence and the bars feel slow (G11). |
| Repo knowledge: common fix directories, file conventions, review preferences (`repo_knowledge`) | Conventions and lessons in `knowledge_entries` under `repository-convention` and `strategy-lesson` | Conventions comparable. Zone has no "where fixes land" entry and **discards** its review classifications (G3). |
| Review comment classification, 7 keyword categories, first match wins, LLM first | `learning::review`: 8 categories, whole-word and phrase weights, evidence bar 0.9 | Zone's classifier is better; its output goes nowhere (G3). |
| Strategy fingerprints, "successful strategies" block (top 3, ordered by a score that is always NULL) | `learning::strategy` + `learning::lesson`: grouped by digest, `>= 3` distinct runs, mean quality `>= 0.6`, capped at 8 | Zone ahead; `strategy::similarity` is dead code (see below). |
| Quality scoring `0.5·speed + 0.3·cycles + 0.2·approvals`, 120-minute half-life, stored on `prs` and never read | `learning::quality`, same weights, exponential 120-minute half-life, bands, **consumed** as the teaching-run gate and the lesson quality | Zone ahead. |
| Temporal issue clusters (30-minute window, `>= 3`) and a cluster sentence in the prompt | None | Not applicable without issue intake; the regression watch covers "the same failure keeps coming back". Skip. |
| AGENT.md generation (off by default) | None; zone reads the repository's own `AGENTS.md` / `CLAUDE.md` and renders learned facts into the prompt directly | Skip the file; the prompt already carries it. |
| Cross-repository correlation (24-hour window, surfaced at `>= 3`) | None | Low value while a workspace is the unit of learning. Skip unless multi-repository workspaces become common. |
| Six prompt blocks in a fixed order, agent runs only | Standing instructions, conventions, lessons, memory, skills, repository instructions, **task runs only** | Chats read none of it (G5). |
| Prioritisation engine ordering the poll queue (`0.30/0.25/0.20/0.15/0.10`) | `services/prioritisation`: five signals (`0.35/0.20/0.20/0.20/0.05`), six blast-radius tiers, suppression rules; **`queue::order` has no caller** | Zone's scorer is richer and unused for the queue (G7). |
| Q&A reuse with success counters (`0.75·cosine + 0.25·success rate`) | None; `ask_user` answers are not reused | Optional (G8). |
| Retrieval quality judge (off by default, fed back nowhere) | None | Skip. |
| Repo-inference feedback (recorded, never read) | Not applicable | Skip. |
| Repetitive-issue weekly digest | `workers/reports` digest with regressions, failure categories, trend; `workers/regression` recurrence and reopen checks | Zone ahead. |
| Self-evaluation gate (before/after tests, lint, static analysis; PR comment; `fail_on_regression`) | `workers/evaluation` measures the same before and after a run and writes a log entry; nothing reads the verdict | Both stop short of learning from it; zone lacks the gate and the PR comment (G9). |
| Operator instructions (`agent_instructions`, not learned) | Standing instructions, knowledge entries, memory, repository files | Equivalent. |
| Nine dedicated tables with typed columns | One table, provenance as tags, no schema constraint, no dedicated endpoints | Zone's storage is the weaker side (G10). |
| `[learning]` config: fifteen keys, per-capability switches | Constants, no switches | G11. |
| Learning dashboard: knowledge, instructions, strategies, review patterns, correlations, feedback tabs | None; learned rows appear as wiki entries with raw tags | **Missing** (G6). |
| Hundreds of unit tests, no end-to-end learning scenario | Unit tests plus out-of-crate threshold pins, no test that crosses the loop | Both lack the crossing test (G12). |

## What zone lacks, and what porting it would take

Each item names the claudear behaviour, why it matters for zone, and the
concrete work in zone's terms: data, worker, consumer, surface, tests. Sizes
are S (a day), M (a few days), L (a week or more).

### G1. Similar past runs in the prompt (M)

Claudear embeds every issue and, for a new one, prepends the learnings and
outcomes of the five most similar past issues. Zone has the ingredients and
not the join: an embedding service, the run outcomes, the strategy digests and
the quality bands, but no embedding of the task.

- Data: a `task_embeddings` table (task id, vector, model, updated at) written
  when a task is created or its title and description change, through the
  same provider the knowledge index uses.
- Worker: none new; embed inline on the task write path, and let
  `knowledge_refresh`'s backlog drain cover failures the way it does for
  knowledge entries.
- Consumer: a fourth guidance block in `workers/task.rs::guidance`, after
  learned facts, listing up to five past runs of this workspace above a cosine
  bar: title, outcome, quality band, approach, and the failure category when
  it failed. Read-only context, budgeted like the memory block.
- Surface: none needed.
- Tests: one for the retrieval bar and cap, one that the block renders into an
  assembled run prompt, and a threshold pin in `learning_thresholds_tests.rs`.
- Not to copy: claudear's `min_sim 0.1`, which is effectively "any five".

### G2. A per-run lesson record (S, optional)

Claudear stores a one-line learning per merged attempt (root cause, files,
strategy, whether tests ran). Zone's strategy fingerprint already captures the
approach and the files; what it does not keep is a sentence a person can
read. The run's own final report is already stored, so the smallest version
is to render the fingerprint (`approach`, `phases`, tests run) into the run's
artifacts and show it in the run view. Skip the regex root-cause extraction;
claudear's own LLM path is off by default and the regex accepts only a
narrow phrasing.

### G3. Keep and use the review classifications (S)

Zone classifies every review comment on a run's pull request into eight
categories with a weighted, whole-word matcher and then throws the tally away
(`LearningOutcome.review`, computed at `learning/worker.rs:181`, never written).
Claudear persists its cruder classification and promotes repeated ones into
the prompt as "review preferences". This is the richest correction signal in
either system and zone already has the better half of it.

- Data: a fourth learned category, `review-preference`, in
  `knowledge_entries`, provenance tags as for conventions (`learned:`,
  `observations:`, `runs:`, `confidence:`, `confirmed:`).
- Worker: in `learning::worker::decide`, tally categories across the
  workspace's runs and write a fact when a category recurs across at least
  three distinct runs (the convention bars, reused), with the statement built
  from the category ("Reviewers here ask for tests with every change").
- Consumer: `learned_facts_prompt` already loops `LearnedCategory::ALL`; add
  the heading and the preamble ("what reviewers keep asking for").
- Tests: the tally-to-fact threshold, and the prompt block.
- Not to copy: claudear re-promotes a pattern on every review because it
  never marks it promoted; zone's upsert-by-fingerprint already prevents that.

### G5. Chats read what the workspace learned (S)

Only task runs receive standing instructions and learned facts
(`workers/task.rs:1226`, `:1240`); a chat about the same repository gets
neither. Claudear has the opposite limit (learning reaches only agent runs
that resolved a repository), so this is less a port than a fix, but it is the
cheapest large gain here.

- Consumer: in `services/chat/session.rs`, after the memory block, render
  `standing_instructions_prompt` and `learned_facts_prompt` for the chat's
  workspace under a byte budget, with the same `READ_FILTER`.
- Tests: an assembled chat prompt carries the blocks; the prompt-size
  ceiling test moves by the measured amount.

### G6. A surface for what was learned (M to L)

Claudear's dashboard has a Learning page with tabs for knowledge, standing
instructions, strategies, review patterns and correlations. Zone's learned
rows are visible only as ordinary wiki entries whose provenance is a list of
raw tag strings, and the category-guarded retirement functions
(`retire_learned_fact`, `retire_standing_instruction`) have no HTTP caller at
all.

- Data: see G10; the surface is much easier once provenance is typed.
- Routes: `GET /api/workspaces/{id}/learned` (conventions, lessons, review
  preferences, standing instructions, each with confidence, observations,
  distinct runs or chats, last confirmed) and
  `POST /api/knowledge/{id}/retire` calling the guarded retirement.
- Console: a "Learned" page under the workspace: one list per category, the
  provenance as fields not tags, a retire action with the reason recorded, and
  the analytics summary the Prometheus gauges already carry (success rate,
  trend, failure categories) so a person sees it without Grafana.
- Tests: route authorisation (member reads, viewer cannot retire), and the
  page's contract test against the route's JSON.

### G7. Use the prioritiser, or remove it (S to M)

`services/prioritisation::queue::order` orders tasks by five weighted signals
and nothing calls it; the task queue is still ordered by priority and queue
time elsewhere. Claudear's equivalent orders its poll queue. Either wire
`order` into task admission (score computed from the task's priority, its
recent failure category and the blast radius of the files its last run
touched) or delete the queue half and keep the pull-request risk signal,
which is used. Leaving a tested, unused subsystem in place is the worst of
the three.

### G8. Reuse answered questions (M, optional)

Claudear matches a new question against past human answers (`0.82` scoped,
`0.88` global) and blends in a success rate that merges and closes feed back.
Zone's `ask_user` answers are stored on the run and never consulted again;
its promotion pass covers only chat exchanges. A port would embed each
question and answer pair on the run, offer the model the top matches as
context before it asks, and count whether the run that reused an answer
succeeded. Worth it once questions recur; not before.

### G9. Learn from the evaluation (M)

Zone measures tests, lint, typecheck and coverage before and after a run
(`workers/evaluation/`) and records the comparison as a log line. Claudear
does the same and also comments the deltas on the pull request and can fail
the attempt on a regression. Neither feeds the result into learning.

- Consumer: add the evaluation verdict to `task_runs.artifacts.evaluation`
  and let `learning::quality` weight it (a change that made the tests worse
  is not a teaching run whatever its merge speed).
- Gate: an optional per-workspace "do not publish on a regression" that
  refuses the pull request step with the deltas in the run's report, and a
  pull-request comment with the deltas as claudear posts.
- Tests: the quality weighting, and a run that regressed is refused
  publication when the switch is on.

### G10. Typed provenance and a schema for learned rows (M)

Every learned fact's provenance lives in `knowledge_entries.tags` as strings
(`learned:{fp}`, `observations:{n}`, `confidence:{0.00}`); a malformed tag
silently drops the provenance line from the prompt, nothing constrains the
reserved categories at the database, and there is no index beyond the generic
GIN on tags. Claudear's tables carry `confidence`, `occurrence_count`,
`source_type` and `is_active` as columns.

- Migration: `knowledge_entries.provenance JSONB` (kind, fingerprint,
  observations, runs, chats, confidence, confirmed) and a partial index on
  `(workspace_id, category)` for the reserved categories, plus a CHECK that a
  reserved category carries provenance. Lock-bounded like migrations 021
  onwards; registered with the installer and the migration census. The column
  holds what the tags encode today, typed:

  ```json
  {
    "kind": "repository-convention",
    "fingerprint": "3f1a9c0e7b2d4e6f8a9b0c1d2e3f4a5b",
    "observations": 6,
    "runs": 4,
    "agreement": 0.83,
    "confidence": 0.78,
    "confirmed": "2026-09-16"
  }
  ```
- Code: `LearningProvenance` and `PromotionProvenance` read and write the
  column, with the tags kept for one release as the fallback.
- Tests: the migration test list, the upsert outcomes, and a row with
  provenance in the column and none in tags still rendering.

### G11. Switches (S)

Claudear exposes a switch per capability and a threshold for the two it
promotes on. Zone's constants are deliberately fixed and pinned by tests, and
should stay so; what is missing is the ability to turn a pass off for a
workspace (a workspace of throwaway experiments should not teach) and to see
the bars. Add `learning_enabled` and `promotion_enabled` to the workspace AI
settings, honoured by `active_workspaces` and `load_exchanges`, and render the
bars read-only on the Learned page.

### G12. A test that crosses the loop (S)

Neither project has an end-to-end test that a learning pass writes a row and
the row lands in a prompt. Zone's threshold pins are precise and the pieces
are pure, so the crossing test is cheap: seed three finished runs with file
changes and a well-received pull request into a test database, run
`learning::worker` once, assert one `repository-convention` row, then
assemble the task guidance and assert the block. The same for promotion with
four exchanges across three chats.

## What zone has that claudear does not

For calibration, the other direction.

- Failure categorisation that is wired, embedding-based, and refuses a near
  tie (`0.05` margin) instead of guessing.
- Quality scoring that is consumed: it gates which runs may teach and weights
  lessons. Claudear computes the same score and never reads it.
- Conjunctive bars everywhere: a convention needs four observations, three
  distinct runs and three-quarters agreement; a lesson three distinct runs
  and a mean quality of 0.6; a promotion four occurrences, three chats and
  cohesion 0.88. Each pinned by a test that fails when the constant moves.
- Provenance and reversible retirement on every learned row, with an upsert
  that reports created, superseded or unchanged and keeps one identity per
  scope when the answer changes.
- Screening of promoted answers through the memory rulebook, so a credential
  or a contact detail never becomes standing.
- A regression watch (recurrence and reopen checks), agent analytics with
  trend detection, and a scheduled digest, none of which claudear has in this
  form.
- A read filter on every learned block: an entry that would suppress an error
  or a disagreement is treated as absent.

## Zone's own broken links

Found while inventorying; independent of claudear, and most are cheaper than
any port above.

1. `LearningOutcome.review` is computed every pass and never written, logged
   or rendered (`learning/worker.rs:181`). G3 fixes it.
2. Chats never read standing instructions or learned facts. G5.
3. `learning::strategy::similarity` is tested and has no caller; lessons
   group by exact digest, so two near-identical approaches make two lessons
   or neither clears three runs. Either group by similarity above a bar or
   delete the function.
4. `retire_learned_fact` and `retire_standing_instruction` have no HTTP or UI
   caller; the only reachable retirement is the generic, unguarded
   `DELETE /api/knowledge/{id}`. G6.
5. No console surface shows confidence, observations or distinct runs; the
   tags render as raw strings on the wiki page. G6 and G10.
6. `regression::Alerted` and `reports::Ledger` are in memory; a restart can
   re-alert a regression or lose one digest slot. A small table, or the
   `task_runs.artifacts` of the task alerted on, fixes both.
7. `queue::order` is unused. G7.
8. The evaluation worker's verdict never reaches learning. G9.
9. No integration test crosses the loop. G12.
10. `db/analytics.rs`, the shared read layer for three capabilities, has no
    tests of its own.
11. `housekeeping/mod.rs:40-50` states several sweeps are not safe on two
    instances; the only protection is the advisory lock inside the upsert.

## What not to copy

Claudear's inventory turned up defects a port would otherwise inherit.

- `prs.fix_quality_score` is written and never read;
  `strategy_fingerprints.fix_quality_score` is always NULL, so the
  "successful strategies" block is ordered arbitrarily.
- `review_patterns.promoted_to_instruction` is never set, so a pattern is
  re-promoted on every review and its knowledge count grows without bound.
- The embedding branch of content clustering cannot run: the caller passes an
  empty embeddings map, so clustering is always Jaccard over titles.
- Every semantic feature depends on the vectorlite SQLite extension and
  silently becomes a no-op without it.
- `EmbeddingConfig::from_env` is never called in production, so the
  documented model choice does not exist; only Nomic v1.5 is reachable.
- The quality scorer's comment says exponential decay; the formula is
  hyperbolic. Zone's is exponential.
- Roughly half the tuning constants are hard-coded despite the `[learning]`
  section that suggests otherwise.

## Suggested order

Small and high-value first, each a pull request of its own.

1. G3, keep the review classifications (S). Code exists; one category, one
   block.
2. G5, chats read the learned blocks (S).
3. G12, the crossing test (S). It de-risks everything after it.
4. G10 then G6, typed provenance, routes and the Learned page (M, then M to L).
5. G1, similar past runs in the prompt (M).
6. G9, evaluation into quality and an optional publication gate (M).
7. G7, wire or remove the prioritiser (S to M).
8. G11 switches (S); G2 and G8 only if the need shows up.

Not taken: AGENT.md generation, issue clustering and duplicate skip,
cross-repository correlation, the retrieval judge and repo-inference
feedback. Each is either not applicable to how zone takes in work, off by
default and unread in claudear itself, or already covered by something zone
renders directly into the prompt.

## Since this review

Added 2026-11-11. Everything above is the comparison as it stood at the two
commits named at the top and is left as written, its numbers included. This
section records what has changed in zone since, where it bears on the review.
The constants and names are the ones in the code on that date.

### The evidence bars moved down one notch

Six bars were lowered and five were kept. Wherever the body quotes a bar, it
quotes the left-hand column.

| Bar | At the review | Now |
|---|---|---|
| Convention, observations (`MINIMUM_OBSERVATIONS`) | 4 | 3 |
| Convention, distinct runs (`MINIMUM_DISTINCT_RUNS`) | 3 | 2 |
| Convention, agreement (`MINIMUM_AGREEMENT`) | 0.75 | 0.70 |
| Convention, confidence (`MINIMUM_CONFIDENCE`) | 0.6 | 0.6, kept |
| Lesson, distinct runs (`MINIMUM_RUNS`) | 3 | 2 |
| Lesson, mean quality (`MINIMUM_MEAN_QUALITY`) | 0.6 | 0.6, kept |
| Promotion, occurrences (`MINIMUM_OCCURRENCES`) | 4 | 3 |
| Promotion, distinct chats (`MINIMUM_DISTINCT_CHATS`) | 3 | 2 |
| Promotion, question similarity (`MINIMUM_QUESTION_SIMILARITY`) | 0.88 | 0.88, kept |
| Promotion, answer similarity (`ANSWER_SIMILARITY`) | 0.85 | 0.85, kept |
| Promotion, answer agreement (`MINIMUM_ANSWER_AGREEMENT`) | 0.6 | 0.6, kept |

Why each kept bar stayed:

- Convention confidence. It is what makes the new minimum count unanimous:
  three observations of three score 0.625 and clear it, three of four score
  0.525 and do not.
- Lesson mean quality. A change nobody has reviewed or merged scores 0.55, so
  any move toward that figure would let two untouched pull requests teach a
  lesson.
- Promotion question similarity. It decides which questions are the same one,
  not how much evidence is needed, and a lower value merges different
  questions.
- Promotion answer similarity. It decides which answers say the same thing,
  and a lower value counts dissent as agreement.
- Promotion answer agreement. Three answers can agree at 0, 0.5 or 1, so 0.6
  holds the minimum count to unanimity and tolerates one dissenter from four
  answers up.

The bars are still conjunctive and each is still pinned by a test that fails
when its constant moves, in the three modules and in
`tests/integration/learning_thresholds.rs`. One limit is unchanged and matters
more at two runs than it did at three: distinct runs are counted, not distinct
tasks, so a task run twice can supply both runs a convention or a lesson
needs.

Three occurrences in two chats is a bar two automations could clear on their
own, so two guards came with it. `load_exchanges` reads only the questions
`db::chats::typed` calls typed: a prompt that a reminder, a watch or another
automation posts under the member's role is not an occurrence, and two watches
asking the same thing in two chats promote nothing. And because chats are
counted, not people, while the block now reaches every member's chat (G5,
below), the standing-instruction preamble says that each entry answers a
recurring question and never by itself permits a write, a send or a command
the user did not ask for.

### G5 is closed: chats read what the workspace learned

A chat's system prompt now carries the standing instructions, the repository
conventions, the strategy lessons and the recorded discoveries described
below. `agent::learned` loads each family, renders it with the renderer that
already sat beside its store, and holds the whole block to
`MAX_RENDERED_BYTES`, 8,000 bytes, the figure the skills index is held to.
Past the bound an entry too large to be shown even alone under its family's
heading goes first; then whichever family currently renders largest gives up
the entry at the end of its list, whole, until the block fits. A family down
to its last entry gives nothing while another still has a second. The block
closes by saying how many entries were left out, the ones past a store's row
limit included, and the discoveries block keeps its own count, so no entry is
counted twice. Every block keeps its `READ_FILTER`.

Two things differ from the sketch under G5. The block sits before the memory
block, not after it, which is the order a task run reads them in. And a
character card with its agent off reads none of it. Each family is read on its
own, so a store that cannot be read costs the turn that family and a warning,
not the block and not the reply. A chat with its agent off also searches the
knowledge store before it answers, and that search now leaves out the entries
the block already shows. Task runs are unchanged: they read the same families,
each held only by its own limit.

### Inline recording: `record_discovery`

The review describes a zone that learns only through its two passes. There is
now a third way in. `record_discovery` is a tool a chat or an authorized task
run calls, unasked, when it has verified something durable about the
repository that the next run would otherwise rediscover. It writes a
`knowledge_entries` row in a fourth reserved category, `agent-discovery`, with
its kind (`convention` or `lesson`), the run or chat it came from and the date
it was recorded in tags, and with the member that run or chat was acting for
as its author. The prompt calls it the one workspace write made
unasked. A chat's memory write of what a person says about themselves is also
made unasked, but it is that person's entry and not the workspace's.

- No bar. One agent's note from one run or one chat is stored as it is, which
  is why it has a category of its own and renders under its own heading,
  `# Recorded while working here`, introduced as leads to check and not
  instructions. It does not render under a pass-derived family's preamble or
  compete with pass-derived facts for their row limit.
- Screened, by a rulebook of its own. `rules::refused_discovery` keeps the
  credential detector but shows it a long run that reads as a name (a
  repository path, a branch, a route, a `SCREAMING_SNAKE` constant) with its
  digits respelt, so the entropy rule does not take a name for a generated
  value. It narrows identifiers to a person's email address and an
  international telephone number, and narrows suppression to an instruction:
  one that opens a clause, or one that says whom something is kept from. What
  the docs do not mention and what a log never tells you are statements, and
  are recorded. The memory rulebook, `rules::refused`, is unchanged and still
  screens the memory tools and the promotion pass; it would refuse what a
  repository note exists to say. A control character or one that does not
  show is refused as well, so a person reviewing the entry sees the text the
  model reads, and so is a call carrying any field beside the three.
- Kept out of the trace when refused. A call the tool is about to refuse for
  its shape, its size or what the screen found leaves only its kind in the
  chat's tool trace, in a run's tool-call record and in the run log, and no
  receipt for the tool carries a stated reason. A call it takes leaves the
  entry it stores there, not the text the model wrote, so the copy of a field
  written twice that no screen read is left out too.
- Bounded in bytes. A subject and its statement are stored with their white
  space collapsed to the one line a prompt renders, and that line holds
  `MAX_ENTRY_BYTES`, 1,000 bytes, for the two together. The rendered block
  holds `MAX_RENDERED_BYTES`, 6,000 bytes, and is read from at most
  `MAX_LISTED`, 117, rows. They come most recent first, and only a source's
  `ALLOWANCE`, its three most recent, are ordered that way: what a run or
  chat recorded beyond it comes after every source's allowance, dealt a
  source at a time. So one chat or one run cannot fill the block, and a run's
  second and third entries are not hidden behind what older sources each
  recorded once; the share is settled at the read, and nothing limits how
  many a source writes.
  Entries that do not fit are left out whole and the block says how many,
  counted against everything recorded.
- One entry per subject. Recording a subject again replaces its entry, which
  is how a stale note is corrected, and the call is answered with the
  statement it wrote over.
- Deletion is final, as far as a fingerprint can tell. A subject whose entry
  was deleted through `DELETE /api/knowledge/{id}` is refused when recorded
  again, and so is its statement under another subject: the entry keeps a
  fingerprint of each, taken past casing and spacing and nothing else, so a
  reworded note is recorded as a new one. The two pass-derived families still
  revive a deleted row when a pass re-derives it from fresh evidence.
- Unconfirmed. The tool is in the write tier, so nobody approves the call in a
  chat or in a run; a plan hold still refuses it. Recording needs member
  standing, read on every call, and a run with no authorized initiator does
  not hold the tool.

This also changes two rows of the capability table as they would read today:
the prompt-blocks row gains a fourth learned block on both surfaces, and the
storage row gains a fourth reserved category, still with provenance in tags
(G10 is untouched).

### Zone's own broken links, revisited

Checked against the code on the date above.

- Item 2 is closed. Chats read standing instructions and learned facts, as
  described under G5 above.
- Item 4 is open as written, with one neighbouring hole closed.
  `retire_learned_fact` and `retire_standing_instruction` still have no HTTP
  or UI caller, and the generic `DELETE /api/knowledge/{id}` is still the only
  reachable retirement. What is closed is the other unguarded path to a
  learned row, which the review did not list: `update_document` matched any
  local row that was not a memory, so a call handed a learned entry's id could
  rewrite it past every bar and screen. It now refuses the four learned
  categories (`standing-instruction`, `repository-convention`,
  `strategy-lesson`, `agent-discovery`), and the document listing and read
  report those rows as not editable. They stay readable and deletable.
- Item 9 is open for the two passes. No test yet runs the learning or the
  promotion pass and finds its row in an assembled prompt. The half from the
  store to the prompt is now covered: `tests/integration/prompt_assembly.rs`
  seeds a standing instruction, a convention and a discovery and finds each
  in the system entry a chat is prepared with, and
  `tests/integration/discovery.rs` takes a `record_discovery` call through to
  the block `learned_facts_prompt` renders for its workspace. Neither joins
  the tool to a chat's reader, so `tests/integration/discovery_crossing.rs`
  does, with nothing between the two but the database. In
  `what_one_chat_records_the_next_chat_and_the_next_run_of_its_workspace_read`
  one chat records through the tool, and the entry is the one line listed in
  the system entry a second chat of the workspace is prepared with and in the
  block a run of it is handed, and in neither for another workspace. In
  `a_deleted_discovery_leaves_the_next_prompt_and_cannot_be_recorded_back_into_it`
  the entry is deleted from the store, leaves both, and is refused when a
  chat records it again.
- Items 1, 3, 5 to 8, 10 and 11 are not addressed by this work.

In "Suggested order", step 2 (G5) is done. Nothing else in that list has
moved.
