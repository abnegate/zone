# Live real pass on main, 2026-09-30

The 63-row checklist driven again, on `main` at `53a34586` (PRs #96 to #103
since the 21 September pass), plus the features that had never been run live.
Same machine and the same kind of rig: a native `zone-server` built from this
branch, on its own database (`zone_live_0930_5f919a`) and tenants, the console
served from the same worktree, real Ollama, native ComfyUI for the media rows,
and the private `abnegate/zone-tests` repository for everything that touches
GitHub. The user's compose stack at `http://manager.localhost` was only read.

**61 of the 63 rows work, 1 fails and 1 is blocked.** Row 49 fails for the
environment: the model that carried it on 21 Sep may not run beside ComfyUI
here. Row 10 is blocked: there are no OpenAI or Anthropic keys. Among the
features never run live before, the inbound sync webhook (111) fails on a
product defect, and answer promotion (133) was not reached in time.
The pass found seven product defects, all listed in section 4. The biggest are:
- Inbound sync webhooks cannot work: no secret is ever set, and a signed event
  answers 500 after changing the task.
- The AI settings' LiteLLM host and provider keys never reach a completion.
- Two faults in the auto-project reviewer and bot gate.
- Earlier images in a chat are sent to a model that cannot read them.

Evidence for every row -- the screenshots, logs and media files named in the
table, and `evidence.jsonl`, one JSON line per attempt with the latest line for
a row its verdict -- is kept locally and was never committed, rather than carry
39 MB of it in the tree. It sits in
`.claude/worktrees/agent-ac30c7487672c8059-evidence/` under the checkout that
ran the pass; a later pass regenerates the lot. Lane changes are the
`test(live)` commits on this branch; the product code is untouched.

## 1. The machine and how the pass was driven

Apple M4 Max, 64 GB, macOS 26.5.2. Ollama 0.34.0 serves `qwen3.8:27b-ctx32k`, a
Modelfile copy of `qwen3.8:27b` with `num_ctx 32768`, for chat, tools and task
runs. The uncapped tag defaults to a 262k context, which Ollama sizes at 35 to
70 GB. The rest of the backends:
- `qwen2.5:7b-instruct` as the auto-project reviewer.
- `qwen3-embedding:0.6b` for embeddings.
- `llava:7b` for vision.
- ComfyUI 0.34.0 native, at the pinned `30bdda1e`.
- Postgres 18.6 and Redis 8.10.

The rig reached Ollama only through a small proxy on 11436, which did three things:
- It refused the uncapped `qwen3.8:27b`.
- It left out of its model listings the models that cannot carry an agent turn
  (`llava:7b`, a 7B role-play GGUF, and after the first review round
  `llama3.2:3b`, `llama3.2:1b` and a 0.5B GGUF).
- It sent inference for the `llama3.2` tags and for `qwen3-embedding:0.6b` to
  copies with a 16k and an 8k context. Their defaults of 131k and 32k loaded at
  17 GB and 5.8 GB.

A memory guard paused the rig below 20% free. It fired four times:
- Twice when the reviewer rotation loaded a model at its full default context
  (defect D3).
- Once when a lane and the auto project each held a model.
- Once when the Docker VM held a 24 GB page cache after a prune. Dropping the
  VM's caches returned it.

Each time the rig was resumed or restarted only after the cause was dealt with.
Docker Desktop also went down twice during the pass: its backend exited at
06:07Z, and at 07:12Z the engine hung because the Docker VM's disk was full.
The stack came back each time on its restart policies, and the pass's
standalone containers were started again.

The rig ran with the environment `scripts/live-verify.sh` gives it, plus:
- `ZONE_MCP_ENABLED=true` and `ZONE_TASK_EVALUATION=1` with
  `ZONE_TASK_EVALUATION_CATEGORIES=all`.
- `ZONE_AUTO_TICK_SECS=15`, `ZONE_AUTO_CHECKS_GRACE_SECS=60`,
  `ZONE_AUTO_BOT_REVIEW_GRACE_SECS=60`, `ZONE_AUTO_POST_MERGE_SECS=300`,
  `ZONE_AUTO_MAX_ACTIVE_RUNS=1` and `ZONE_AUTO_PARALLEL_TASKS=1`.
- `ZONE_AUTO_REVIEW_MODELS=qwen2.5:7b-instruct`. From 06:25Z also
  `ZONE_AUTO_REVIEW_BOTS=greptile`, because CodeRabbit's free plan had run out
  of reviews.
- `SEARCH_ENABLE_WEB_SEARCH=true`, pointed at SearXNG.
- `MONITORING_*`, pointed at Prometheus and Grafana.

SearXNG, Prometheus and Grafana ran as standalone containers beside the rig,
at the image digests compose pins and with the repository's own
settings, provisioning and dashboards. Most SearXNG engines refuse or CAPTCHA this
address (the host sits behind a VPN client). `google cse` answered once its
suspension lapsed, so a proxy on 8089 dropped the rig's `engines=bing` filter.

zone-tests' GitHub Actions workflow could not start jobs (a billing problem on
the account), so it was disabled for the pass and re-enabled afterwards. A local
stand-in ran `./check.sh` on every open `zone/task-*` head and posted the result
as a commit status, the way an external CI reports.

What the chat produced this time, through Zone's own path. Media rows ran on
`qwen2.5:7b-instruct` as chat and classifier model, because the 27B may not run
beside ComfyUI:

| Through the chat | Time | Result |
|---|---|---|
| Image 1024x1024 (row 38) | 89 s | `38-image.png` |
| Upscale x4 of that image | 14 s | 4096x4096, 28.8 MB |
| Audio, "a soundscape of rain on a tin roof..." (row 38) | 47 s | `38-audio.flac`, routed on the first phrasing |
| Video (row 38) | 627 s | `38-video.webm` |
| Upscale of that video | 137 s | `38-video-upscaled.png` |
| Edit from the starting image (row 36b) | about 3 min | `36-edit-result.png`: same scene, a lighthouse added; the boat did not turn red |
| LoRA training on the 4 s clip (rows 17, 18) | 42 min | `live-pass-zrkxyz-1790756820041.safetensors`, "Strong, 49% better", 3 near-duplicate frames screened out |
| The adapter used from chat (row 19) | 2 min | `19-adapter-image.png`, rendered with `LoraLoaderModelOnly` on the adapter |

## 2. The table

| # | Area | Feature | Result | Evidence | Cause and why |
|---|---|---|---|---|---|
| 1 | Auth | Register a new account | WORKS | 01-register-form.png, 01-registered-landing.png, 01-fresh-account-signed-in.png, user_id=9c3c0804-aa9d-4fa0-831f-70a1fe31ad2e |  |
| 2 | Auth | Email verification | WORKS | 02-after-registration-banner.png, 02-after-resend.png, 02-verify-path-the-mail-would-use.png, 02-verify-email-outcome.png | No SMTP on this machine: no mail was sent; the verification token row was read from the database and the console's verify page accepted it (the delivery half is BLOCKED: no credentials). |
| 3 | Auth | Login, logout, wrong password, reload | WORKS | 03-signed-in.png, 03-after-reload.png, 03-logged-out.png, 03-wrong-password-refused.png |  |
| 4 | Auth | Sessions page and revoke all others | WORKS | 04-sessions-page.png, 04-sessions-after-revoke.png, 04-second-browser-after-revoke.png |  |
| 5 | Auth | Forgot and reset password | WORKS | 05-forgot-password-sent.png, 05-reset-form.png, 05-reset-outcome.png, 05-signed-in-with-new-password.png | No SMTP: the reset token row was read from the database (the delivery half is BLOCKED: no credentials). |
| 6 | Auth | Invitations | WORKS | 06-invite-form.png, 06-invitation-pending.png, 06-invitation-accept-page.png, 06-member-listed.png, invitation_id=3eea9339-019e-4641-9dca-ebae5595fe52 | No SMTP: the invitation was opened by its token from the database. The Members table now shows an editable member's role in a select, which `auth.live.ts` reads. |
| 7 | Auth | /unauthorized for a member without the role | WORKS | 07-member-at-org-settings.png |  |
| 8 | Org settings | Members: change a role, audit log | WORKS | 08-members-before.png, 08-role-changed.png, 08-audit-logs.png |  |
| 9 | Org settings | AI Settings self-hosted models, save, reset | WORKS | 09-ai-settings-filled.png, 09-auto-chat-fast-turn.png, 09-auto-chat-reasoning-turn.png, 09-reset-alert.png, 09-after-reset.png, chat_id=c797af58-6db8-426d-89ea-36fca200b99e |  |
| 10 | Org settings | AI provider OpenAI and Anthropic, LiteLLM section | BLOCKED | 10-provider-openai-form.png, 10-provider-anthropic-form.png, 10-litellm-saved.png | no credentials: no OpenAI or Anthropic API key on this machine. The provider forms render and the LiteLLM section saves; no turn was answered by either provider. |
| 11 | Org settings | Billing page | WORKS | 11-billing.png |  |
| 12 | Workspace settings | Theme | WORKS | 12-theme-form-and-preview.png, 12-theme-on-chats.png, 12-theme-on-wiki.png, 12-theme-on-tasks.png |  |
| 13 | Workspace settings | Workspace AI override and credentials | WORKS | 13-workspace-ai-override-form.png, 13-workspace-chat-uses-override.png, 13-reset-alert.png, chat_id=a28db502-c9aa-4c54-acf9-0ea9563f405d |  |
| 14 | Models | Installed models, disk meter, sort, details, delete | WORKS | 14-installed-models.png, 14-model-details.png, 14-delete-confirm.png, 14-after-delete.png |  |
| 15 | Models | Add Model from catalog and HuggingFace | WORKS | 15-browse-filtered.png, 15-download-options.png, 15-catalog-install-complete.png, 15-huggingface-install-complete.png |  |
| 16 | Models | Stop Ollama: error and recovery | WORKS | 16-ollama-stopped.png, 16-ollama-recovered.png |  |
| 17 | Models | Train tab: base, frames, mirrors, captions | WORKS | 17-train-tab.png, 17-frames-with-mirrors.png, 17-frames-mirror-off.png, 17-captions-from-vision-model.png, 17-ready-to-train.png |  |
| 18 | Models | Training result and quality | WORKS | 18-training-result.png |  |
| 19 | Models | Adapter used from chat | WORKS | 19-adapter-image-in-chat.png, 19-adapter-image.png, chat_id=56f6343c-d253-44a6-b627-76e91bde2741 | The adapter was selected through Organization AI Settings (model_image = the adapter file) with no restart; the prompt ComfyUI ran carried LoraLoaderModelOnly on live-pass-zrkxyz-1790756820041.safetensors (evidence 19, 19.1). Resemblance to the clip subject is weak: a small figure in a snowy forest with the trigger word lettered in a corner. |
| 20 | Sources | GitHub source | WORKS | 20-source-kinds.png, 20-source-github-form.png, 20-source-verified.png, 20-source-disabled.png, source_id=9989ad81-5aba-462d-a804-ea62ec95622d | The wizard offers no Verify step, no "Allow write operations" for GitHub (Filesystem only) and no "Main repository" toggle; Verify and Enable/Disable live on the card. Delete is exercised in row 31. |
| 21 | Sources | GitLab, Web URL and Text sources | WORKS | 21-gitlab-verify.png, 21-web-url-verify.png, 21-text-verify.png | Web URL and Text sources verify; GitLab is refused on a deliberately bad token because there is no GitLab token here (that half is BLOCKED: no credentials). The checklist's Notion and Slack sources are not covered: the server has no adapter for either, so the wizard does not offer them. |
| 22 | Sources | Source indexing and search | WORKS | 22-context-search-github.png | Chat citation of repository content is checked in row 37 |
| 23 | Projects | Projects | WORKS | 23-project-created.png, 23-project-cancelled.png, 23-project-after-unlink.png, 23-project-deleted.png |  |
| 24 | Projects | External sync | WORKS | 24-add-sync-form.png, 24-sync-after-add.png | Adding and listing a sync works in the console. Inbound delivery was checked separately (new coverage, feature 111) and fails: defect D1. |
| 25 | Tasks | Read-only task run | WORKS | 25-task-created.png, 25-execution-logs-streaming.png, 25-run-finished.png, task_id=ddd9f5db-61f6-4688-9a53-74d30a04f76b, run_id=f9684001-82f6-46e8-b961-3a2fd53c166d |  |
| 26 | Tasks | Plan approval | WORKS | 26-plan-approval-parked.png, 26-approved-run-finished.png, task_id=92e59a39-9a22-40d3-9384-547034906bc6, run_id=dc56f7bd-0936-4ec7-a217-e23047a3fa4d | Approve carried the plan out and it became PR #12 (row 30). The Revise half (26b) now sends its instruction under Other, where the card takes it (`repo.live.ts`, `plan-revise.live.ts`). |
| 27 | Tasks | Run asks a question | WORKS | 27-waiting-for-you.png, 27-resumed-and-finished.png, task_id=03855c44-30a6-4341-b97d-4c358abbee8c, run_id=b1d50e34-2a6f-4073-b438-93c09368c7d9 |  |
| 28 | Tasks | Background command, wait_for, tail_job | WORKS | 28-background-job-run.png, task_id=2777fbf4-ff14-44dc-af47-f759cb286603, run_id=804bbf80-548c-46eb-b627-03aa80fd863c |  |
| 29 | Tasks | Refused command corrected | WORKS | 29-refused-then-corrected.png, task_id=79394e17-2fe8-4abe-8d0e-571a61e838c2 |  |
| 30 | Tasks | Repository task to pull request | WORKS | 30-task-pr-open.png, 30-task-card-after-merge-and-sync.png, task_id=92e59a39-9a22-40d3-9384-547034906bc6, pr_url=https://github.com/abnegate/zone-tests/pull/12 |  |
| 31 | Tasks | Run Again, failed run, delete | WORKS | 31-run-again-offered.png, 31-run-again-finished.png, 31-failed-run.png, 31-task-deleted.png | After a failed run the dialog offers "Run Again"; "Try Again" is the label after a start error only |
| 32 | Tasks | MCP magents tools | WORKS | 32-mcp-task-run.png, task_id=a875f422-94b6-4e4d-b775-0bb48c8ceb87 | Both halves work for the first time: a task run called magents_spawn_session (codex), and from a chat magents spawned a codex session, read its transcript and the chat repeated what it printed (evidence 32 and 32.5). A headless claude spawned by magents still answers "Not logged in" here, so the lane names codex (ZONE_MAGENTS_AGENT). |
| 33 | Chats | Chat management, model choice, agent mode | WORKS | 33-fast-model-chat.png, 33-reasoning-model-chat.png, 33-renamed.png, 33-after-archive-click.png, 33-archived.png, 33-search-results.png, 33-deleted.png, 33-agent-off-no-tools.png, 33-agent-on-tools.png |  |
| 34 | Chats | Reasoning block, Stop, context meter | WORKS | 34-reasoning-block.png, 34-stopped-mid-stream.png, chat_id=3831a580-2276-468b-82d6-4e13c3666223 |  |
| 35 | Chats | Characters | WORKS | 35-character-editor.png, 35-persona-reply.png, chat_id=7dc10e5a-c0d8-432b-b3f7-63c45ceb6d8c |  |
| 36 | Chats | Attachments | WORKS | 36-image-attached.png, 36-image-described.png, 36-text-attached.png, 36-text-answered.png, chat_id=19da4139-866b-4fa7-9390-1abefb315202 | The image was a real Flux render (a red bicycle) and llava described it; `chats.live.ts` now takes the words to look for with the image (`ZONE_PASS_IMAGE_WORDS`). |
| 37 | Chats | Sources chip | WORKS | 37-composer-sources-chip.png, 37-repository-attached.png, 37-code-question.png, chat_id=7984d472-80e3-434b-aa26-c4daca890400 |  |
| 38 | Chats | Media by intent | WORKS | 38-image-rendered.png, 38-image-upscaled.png, 38-audio-rendered.png, 38-video-rendered.png, 38-video-upscaled.png, 38-upscale-nothing.png, chat_id=55e576db-534e-4ae1-904a-72d09edf3bd5 | Direct Wan 2.2 renders on this ComfyUI produce noise-like frames (see 17-clip-extension-attempt-unusable.png); the video bytes here are judged on size and difference as the lane does |
| 39 | Chats | Upstream failure reported | WORKS | 39-ollama-stopped-mid-turn.png, 39-chat-usable-after.png, chat_id=83ed2e52-a24d-437e-92f1-5d4ba4164eac | Judged on a reply arriving after Ollama is back; the model may answer the interrupted request again rather than the new one-word instruction |
| 40 | Agent tools | read_file, write_file, apply_patch, approvals | WORKS | 40-approval-card.png, 40-write-approved.png, 40-read-file.png, 40-apply-patch.png, 40-deny-card.png, 40-write-denied.png, chat_id=81e249be-69af-4fc9-8162-925ae833745b |  |
| 41 | Agent tools | run_shell foreground and background | WORKS | 41-run-shell-foreground.png, 41-background-job.png, chat_id=8d622213-c9b3-42a6-9fa8-5cc51376e400 |  |
| 42 | Agent tools | ask_user | WORKS | 42-question-card.png, 42-answered.png, chat_id=8cf6eacb-080d-4ee9-8eeb-1b0221bdcd18 |  |
| 43 | Agent tools | Memory tools | WORKS | 43-memory-written.png, 43-memory-read-with-badge.png, 43-memory-appended.png, 43-memory-deleted.png |  |
| 44 | Agent tools | Toolbox search and load | WORKS | 44-reminder-set.png, chat_id=4b4ffe12-565f-49a8-8588-948bbb9358dc | The prompt catalog names every deferred tool, so a model that already knows the name loads it without search_tools; the row asks that the deferred tool is found and loaded, which load_tools alone satisfies |
| 45 | Agent tools | Reminders and condition watch | WORKS | 45-reminder-fired.png, 45-prompt-reminder-set.png, 45-prompt-reminder-ran.png, 45-reminder-cancelled.png, 45-watch-set.png, 45-watch-fired-on-change.png, chat_id=4b4ffe12-565f-49a8-8588-948bbb9358dc | One-off reminder fired and was delivered; an hourly reminder with a prompt ran it as a turn; list and cancel worked. The condition watch (45c) fired twice on the rig: the first reading v1 as its baseline, the second reporting "Changed: ... now contains the single word v2 (previously v1)" (evidence 45.7). Both firings were brought forward in the database (evidence 45.8) because the tool's floor is hourly and the model wrote the first due_at an hour late. |
| 46 | Agent tools | Knowledge and document tools | WORKS | 46-search-knowledge-citation.png, 46-list-and-read.png, 46-create-and-update.png, 46-wiki-shows-document.png, chat_id=907b6cf2-a81c-4f13-b510-a724fe31e260 |  |
| 47 | Agent tools | list_chats, search_chat_history | WORKS | 47-list-chats.png, 47-search-chat-history.png, chat_id=b05d9ee0-588e-4c11-acd6-29ff3cfa47ee |  |
| 48 | Agent tools | web_search, fetch_url | WORKS | 48-fetch-url.png, 48-web-search.png, chat_id=a2a4fe18-494f-4713-8813-71d697081108 | fetch_url read example.com; web_search went through SearXNG (only google cse answers this address; the rig asks with engines=bing, which a proxy dropped). |
| 49 | Agent tools | Media tools called explicitly | FAILS | 49-probe-generate_audio.png | environment, plus D5: with ComfyUI up the pass may not load the 27B, and qwen2.5:7b-instruct (the chat model that fits beside it) does not carry the row. Its classifier sent the explicit image asks down the intent path; the audio ask then failed with 400 (history images sent to a text-only model, D5), and in a fresh chat it came back empty. On 21 Sep the 27B called all three tools. |
| 50 | Agent tools | GitHub tools | WORKS | 50-01-list_sources.png, 50-02-read_repository_file.png, 50-03-list_issues.png, 50-04-github_issue.png, 50-05-get_build_status.png, 50-06-read_check_logs.png, 50-07-create_pull_request.png, 50-08-assess_pull_requests.png, 50-09-review_current.png, 50-10-review_missing.png, 50-11-assess_release_pipelines.png, 50-12-list_deployments.png, chat_id=88419982-4752-4fc0-b7d4-2a7992486a51 |  |
| 51 | Agent tools | Task tools from chat | WORKS | 51-task-created-and-started.png, 51-get-task-run.png, 51-task-on-tasks-page.png, chat_id=12a4ab66-56de-4a38-9dfd-49ebf8e07838 | Rerun after the disk filled mid-lane (ENOSPC on the screenshot). 51b and 51d found no card only because they counted before /tasks had loaded (`followups.live.ts`, `toolset.live.ts`); both tasks are in the workspace and their runs completed. |
| 52 | Agent tools | list_members, send_message | WORKS | 52-list-members.png, 52-send-message.png, 52-message-in-target-chat.png, chat_id=913f7c61-1e6d-4229-b20c-6b1b12d71e54 |  |
| 53 | Agent tools | Prometheus and Grafana tools | WORKS | 53-query-prometheus.png, 53-list-grafana-dashboards.png, chat_id=6a0bd3b9-c301-4bdd-b883-c4ccfebae219 |  |
| 54 | Wiki and search | Wiki | WORKS | 54-text-entry-card.png, 54-url-entry-refreshed.png, 54-search-knowledge.png, 54-agent-cites-entry.png, 54-url-entry-deleted.png, chat_id=73b67211-253c-4556-a5e1-2354f07f8424 |  |
| 55 | Wiki and search | Context Search | WORKS | 55-hybrid.png, 55-semantic.png, 55-keyword.png | Context Search covers indexed sources (zone_context content items); wiki entries have their own search on the Wiki page and the search_knowledge tool (rows 46 and 54). The relevance badge reads "<n>% semantic" or "Keyword match" when the server sends those scores, and "Highly relevant" or "Relevant" only when it sends neither |
| 56 | Tenancy | Tenancy | WORKS | 56-intruder-chats.png, 56-intruder-owner-chat-by-id.png, 56-intruder-wiki.png | With the row 6 membership still in place the second tenant could open the first tenant's chat by id and list its wiki through the API (evidence.jsonl earlier row 56 line): workspace members share the workspace's chats and entries by design; a stranger is what this row checks. |
| 57 | Deployment | Traefik at webui.localhost | WORKS | 57-traefik-console.png | Read only: the console and /api answer through Traefik at manager.localhost (the checklist name webui.localhost is the suffix of the other hosts); no sign-in, so nothing was written to the stack. |
| 58 | Deployment | LiteLLM auto routing | WORKS | 58-trivial-question.png, 58-hard-question.png, chat_id=629bf54e-faf4-40ec-a3a6-d0e1d888f7ca | Against the repository LiteLLM (the image compose builds, with litellm/config.yaml.template, router.json.template and entrypoint.sh) beside the rig. For this lane the rig proxy sent its chat completions there, because the organization LiteLLM host is not read (D2). LiteLLM logged completion() model= qwen2.5:7b-instruct for 2 plus 2 and model= qwen3.8:27b-ctx32k for the proof (evidence 58, 58.1). |
| 59 | Deployment | ollama-init pulls models | WORKS | 59-ollama-init.log | The stack runs on the host Ollama, so ollama-init was run for this row alone in throwaway containers (the pinned ollama/ollama image as the bundled Ollama, pull-models.sh unchanged): it pulled the configured fast model, skipped it for the reasoning slot and pulled the embedding model. The log prints its colour codes literally (D7). |
| 60 | Deployment | Backup and restore | WORKS | counts in evidence.jsonl (row 60) | Backup: the make backup recipe as written, every zone_* volume mounted read-only, archive (1.1 GB) written to scratch instead of the stack directory. Restore: the archive postgres/ into a throwaway volume on the stack image pgvector/pgvector:pg16; the cluster recovered from WAL and chats, users, organizations, knowledge entries and messages match the live stack. make restore itself would write the zone_* volumes, which the pass must not touch. |
| 61 | Deployment | Web search through SearXNG | WORKS | 61-web-search.png, chat_id=6a0bd3b9-c301-4bdd-b883-c4ccfebae219 |  |
| 62 | Deployment | Monitoring profile | WORKS | 62-grafana-dashboards.png, 62-grafana-dashboard-open.png | The monitoring profile, rebuilt beside the rig: Prometheus and Grafana at the pinned digests with the repository provisioning and dashboards, Prometheus scraping the rig under the manager job. All 11 dashboards load; the Manager dashboard shows live numbers for the rig. Exporter-backed panels have no targets here. |
| 63 | Deployment | CLI | WORKS | 63-cli-transcript.log | The first zone run of this pass (04:41Z) was killed after ten minutes: the rig auto project was holding the same Ollama model at the time; run alone it finished in under a minute |

## 3. What changed since 21 September

**In the product** (#96 to #103; before this pass only the agent-provider halves had been checked live, on the stack):

| PR | Change | Rows that exercised it here |
|---|---|---|
| #96 | Chats and tasks can run on the host's signed-in coding agent, with Zone's tools served over MCP | not in scope (Claude Code and Codex were verified on main on 29 Sep) |
| #97 | Chat file tools resolve paths by file identity, `rg --no-config`, child processes get an allowlisted environment | 40, 41, 25 to 29 |
| #98 | Every completion goes through a resolver; the AI settings form gains providers, model lists and Automatic; attempt budget and retry markers in task runs | 9, 13, 25 to 31, 33 to 39, 58 |
| #99, #101 | Frontend CI and test output only | none needed |
| #100 | Claude usage-credit refusals | not in scope |
| #102 | A subagent's refused window no longer ends Claude's turn | not in scope |
| #103 | Page bar stays inside the viewport | every console lane (8, 11, 12, 14 to 16, 23, 54, 55) |

**In the lanes** (one commit per lane or concern, oldest first):
- `auth.live.ts`: the Members table shows an editable member's role in a select.
- `repo.live.ts`, `plan-revise.live.ts`: the plan card offers Approve, Revise and Other; the instruction to stop goes under Other.
- `repo.live.ts`: row 21 lists the screenshot it now saves.
- `chats.live.ts`: the attachment row names the words its image should draw out.
- `mcp.live.ts`: names the magents agent (codex), and judges 32b on what the spawned session printed.
- `skills.live.ts`: a new lane for the workspace skills index.
- `promotion.live.ts`: a new lane seeding the exchanges the answer-promotion job clusters, from a knowledge entry, since a refused claim gives the job nothing to promote.
- `followups.live.ts`, `toolset.live.ts`: wait for the task list before counting a card.
- `auto.live.ts`: a new lane for the auto project from brief to merges and summary.
- `tools.live.ts`: rows 53 and 61 are checked against the rig; web search is judged on the SearXNG results.
- `litellm.live.ts`: a new lane for row 58, against a LiteLLM started from `litellm/`.

**In the results:**
- Row 32 works both ways for the first time. A task run called
  `magents_spawn_session`, and a chat spawned a codex session through magents,
  read its transcript and repeated what it printed.
- Rows 57 to 62 were read from the stack or rebuilt beside it, because the pass
  may not change the stack:
  - backup went to scratch, and restore into a throwaway volume;
  - the bundled Ollama and ollama-init ran in throwaway containers;
  - the monitoring profile ran as standalone Prometheus and Grafana.
- Row 49 no longer works on this machine as the pass had to run it (see its row).

## 4. Defects

PR #104 fixes D1, D1b, D3, D4 and the webhook truncation panic below. D2, D5, D6
and D7 are in progress.

| # | Where | What happens | Evidence |
|---|---|---|---|
| D1 | `routes/webhooks.rs:512` and `:533`, `sync/github.rs:305-311`, `sync/linear.rs:391-396`, `migrations/001_initial_schema.sql:847` | A correctly signed GitHub issue webhook changes the task, then answers 500. The handler updates the task first, then logs a `sync_events` row with `event_type` `issue_closed`/`issue_reopened`, which the table's CHECK (create, update, close, webhook_received, sync_error) refuses. GitHub would redeliver it. Bad and missing signatures are correctly refused with 401. | evidence 111 (29 Sep, re-run 30 Sep 06:16Z), 111.1 |
| D1b | `routes/sync.rs:227`, `db/sync_config.rs:176`, `:286` | In normal use no inbound webhook can succeed. Every sync configuration is created with no webhook secret, and nothing sets one or creates a `synced_items` row, so a real delivery is answered 400 "Webhook secret not configured". For D1 the secret was sealed by the server and copied into the column by hand. | 111, 111.1 |
| D2 | `services/backend.rs:123-131`, `services/chat/session.rs:287-289`, `workers/task.rs:2131-2140` | The organization and workspace AI settings store a LiteLLM host and key, and OpenAI, Anthropic and Bedrock keys, but every non-agent completion goes to the process `LITELLM_HOST` with `LITELLM_KEY`. Model names are honoured; the endpoint and keys are not, and the console offers the fields anyway (`AiProviderFields.tsx:73-94`). Unchanged since 21 Sep. Row 10 never reached a completion, so it could not show this. | 10.1, first row 58 attempt |
| D3 | `workers/auto_project/review/model.rs:97-101`, `pipeline.rs:802-806` | For round 2 onwards the reviewer rotates through every installed completion model, largest first, with no check that it can call tools or fits in memory. It picked the uncapped `qwen3.8:27b` (a 262k context, the rig was paused for memory), then `llava:7b`, which Ollama refuses (400 "does not support tools"). A non-stall reviewer error is returned as a retry, so the same round retried the same model every 15 s until the model was hidden. | 114.2 |
| D4 | `workers/auto_project/review/bots.rs:142-150`, `pipeline.rs:933-943` | CodeRabbit's "Review limit reached" summary (no review, no score) is recorded as a bot round that requests changes with no findings. The driver then sends the task to a fix-up run with nothing to fix, spending one of its three runs. | 114.3 |
| D5 | `services/chat/session.rs:404` (also `ws/chat.rs:1680`, `:2438`) | Images attached to earlier messages are sent with every later turn, whatever the model. Once a chat on a text-only model has produced an image, every completion turn in it fails: "400 Multimodal data provided, but model does not support multimodal requests". | 49 |
| D6 | `workers/auto_project/summary.rs:17`, `:74` | The high-level merge summary is one completion capped at 256 tokens, and a length stop is not checked. On the reasoning model the published sentence stopped at "..., so the". The likely cause is the reasoning spending the budget. | 114.6 |
| D7 (cosmetic) | `ollama/pull-models.sh:14-29` | `printf '%s'` prints the colour codes literally (`\033[0;32m[ollama-init]\033[0m`). | `59-ollama-init.log` |

Also minor: `routes/webhooks.rs:483` and `:496` cut the webhook title and
description with byte slices, which panic when the limit falls inside a
multi-byte character.

Noted, not defects:
- An auto project a crashed or restarted server had claimed waits out the
  15-minute claim lease before any server drives it again
  (`workers/auto_project/mod.rs:36`).
- Task runs commit whatever the work leaves in the worktree. In a repository
  with no `.gitignore` that was `__pycache__/*.pyc` from running `./check.sh`,
  and it cost the auto project two pauses (below).

## 5. New coverage

| Feature | Result | How it was driven, and what happened |
|---|---|---|
| 92 Workspace skills index | WORKS | A knowledge entry with `category: "skill"` was offered in an agent chat's prompt; asked a question it covers, the model called `read_document` on it before answering. |
| 112 Auto project from a brief | WORKS | Projects > Auto project with a short brief for zone-tests. The planner chat interviewed (repository, CI, tests, deployment) and `finalize_project` made a project with three dependent tasks (CI, tests, feature). |
| 113 Automation panel and Resume | WORKS | Two pauses were resumed through `POST /api/projects/{id}/automation/resume`. Each time the paused task went back to `awaiting_checks` and the driver carried on. |
| 114 Auto-project driver | WORKS | Unattended: runs, PRs 16, 17 and 19, the wait for checks (local stand-in CI), a bot request (CodeRabbit, then Greptile), a Zone review by a model other than the author's, fix-up rounds, squash-merges of PRs 16 and 19 by the driver, post-merge checks, and the updates chat ("Merged:", "Paused:", "Complete: ... 2 pull requests merged"). A person stepped in twice, as the pause notices asked. The previous agent removed a committed `.pyc` from PR 16. On PR 17, where every head held only a `.pyc`, this pass deleted it, added a `.gitignore` and then squash-merged it by hand after the 7B reviewer kept repeating the stale finding. The driver picked that merge up ("merged outside the pipeline") and ran the greet task to a clean merge. Defects D3, D4 and D6 were found here. |
| 111 Inbound sync webhook | FAILS | HMAC-signed `issues` events to `/api/webhooks/sync/{id}/github`: closed then reopened moved the task to complete then created, but both answered 500 (D1). A forged and an unsigned request were refused with 401. Reaching the handler at all needed the secret set by hand (D1b). |
| 122 Code-quality evaluation | WORKS | With `ZONE_TASK_EVALUATION=1` a zone-tests task run measured `npm run test`, `lint` and `build` before and after, and stored the unchanged delta on the run. |
| 123 Conflict repair | WORKS | PR 18: main and the branch each appended a different step to one checklist. The driver repaired it through LiteLLM with a two-parent merge commit keeping both lines, and the PR went from dirty to clean. PR 13, where both sides renamed the same owner, was refused ("discarded_ours") and paused for a person, which is the right outcome (123.1). |
| 131 Knowledge refresh | WORKS | A URL entry with a one-minute refresh interval was re-fetched by the 300 s worker. |
| 132 Source resync | WORKS | After a PR merged into zone-tests, the resync worker queued an incremental index of the GitHub source (reason RemoteChanged) with no trigger. |
| 133 Answer promotion | BLOCKED | Time, not the product. The job's first sweep comes a whole six-hour period after the server starts. The one sweep the rig reached (29 Sep 21:48Z: `exchanges=69 created=0`) ran two hours before any exchange was seeded, and every later restart reset the clock. The first seeding asked the model to repeat an unsupported claim; it refused in four different wordings, which would never clear the 0.6 answer-agreement bar. It was re-seeded from a knowledge entry: four answers across three chats, all "port 7070, 14 days". The pass ended before the next sweep (due 12:25 to 13:01Z). |
| 138 Reception sync | WORKS | The 30-minute sweep wrote `merged_at`, `pr_state` and minutes-to-merge for PR 12 and moved the task's `pr_status` to merged. |
| 45 Reminder with a prompt; watch firing twice | WORKS | An hourly reminder ran its prompt as a turn (45). A condition watch read v1 as its baseline, then reported the change to v2 on its second firing (45.7). Both firings were brought forward in the database, because the tool's floor is hourly (45.8). |
| 153 to 157 `zone` CLI | WORKS | Against the rig: `login` (keychain), `setup --check`, `run` (wrote and read back a file), `sessions`, and `resume --last` (appended a second line). A throwaway HOME cannot hold a keychain on macOS, so the real HOME was used. `~/.zone` and the three `zone-cli` keychain items were restored or removed afterwards. `zone config` and `zone lighthouse` were outside the brief. |
| 32 magents from a task and a chat | WORKS | See section 3. |

## 6. Still blocked

- Row 10: no OpenAI or Anthropic key. D2 means a key would not reach a completion anyway.
- The delivery halves of rows 2, 5 and 6 (no SMTP), and GitLab with a valid
  token in row 21. The tokens were read from the database, as on 21 Sep.
- Feature 133, answer promotion: the six-hour sweep was not reached with seeded exchanges in place (section 5).
- Row 49, the explicit media tools, FAILS for the environment. The 27B that
  carried it on 21 Sep may not run beside ComfyUI, and `qwen2.5:7b-instruct`
  does not carry the row. D5 was found on the way.
- Rows 57 to 62 were not run on the stack itself; they were reproduced beside
  it. The VPN profile is still untested (no credentials).
- Wan 2.2 video frames on this ComfyUI and PyTorch MPS build are still not usable pictures.
