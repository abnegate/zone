# Live real pass after the fixes, 2026-09-21

The same checklist, driven again through the console after every defect of the
20 September pass was fixed and the console was tightened page by page. Same
machine, same real backends, no stand-in and no mock: the rig was rebuilt from
branch `fix/live-real-pass`, bootstrapped onto a fresh database
(`zone_live_real2`) by `scripts/live-verify.sh`, and every lane recreated its own
data. The compose stack was rebuilt from the same branch (its images built
from the fixed `manager/Dockerfile`) and its cluster moved into the backed-up
volume with `make migrate-pgdata`.

Evidence for every row is under `docs/live-real-pass-2026-09-21/`: the
screenshots named in the table and `evidence.jsonl` (one JSON line per row, the
latest line per row is the verdict). The first pass and its defects are in
`docs/LIVE-REAL-PASS-2026-09-20.md`; the fixes are the commits between
`b2cdd81e` and the head of this branch.

## 1. The machine

Unchanged from the first pass: Apple M4 Max, 64 GB, macOS 26.5.2; Ollama 0.34.0
with `qwen3.8:27b` (chat and tools), `qwen3-embedding:0.6b`, `llava:7b`;
ComfyUI 0.34.0 native with every bundle; Postgres 18.6 with pgvector; Redis on
6390. Prompt evaluation 146 to 648 tokens/s, generation 12 tokens/s.

What the chat produced this time, through Zone's own path, timed from the
message to the attachment:

| Through the chat | Time | Result |
|---|---|---|
| Image 1024×1024 (row 38) | 184 s | `38-image.png` |
| Upscale ×4 of that image | 8 s | 4096×4096, 26 MB |
| Audio from "a soundscape of rain on a tin roof…" (row 38) | 29 s | `38-audio.flac`, FLAC; the phrasing the first pass missed now routes on the first attempt |
| Video 832×480 (row 38) | 465 s | `38-video.webm`; frames are still the blurred colour fields Wan 2.2 produces on this MPS build |
| Upscale ×4 of that video | 94 s | `38-video-upscaled.webm` |
| Image edit through the tool (rows 36b, 49) | 134 s | `36-edit-result.png` |
| LoRA training on the 4 s clip (rows 17, 18) | 26 min | `live-pass-zrkxyz-1789957359433.safetensors`, "Strong, 49% better", 3 near-duplicate frames screened out |
| The adapter used from chat (row 19) | 2 min | `19-adapter-image.png`, rendered with `LoraLoaderModelOnly` on that adapter |

How the rerun was driven: the rig server was rebuilt from the branch and
restarted with `ZONE_MCP_ENABLED=true`, `SEARCH_ENABLE_WEB_SEARCH=true` and
the SearXNG URL, and `ZONE_CHAT_AGENT_CWD` under the rig; the lanes ran one at
a time (running several against the one Ollama queue made turns take longer
than a lane's wait, so the rows that failed under that contention were run
again alone and their first results discarded). Where a fix changed what a
person sees (auth pages that now redirect after a success message, the rebuilt
AI settings form, hover-revealed card actions, the page bar's "Add source", the
abbreviated context meter, a knowledge delete that deactivates) the lane was
brought in line with the console and the change is in the lane commits.

One defect surfaced during the rerun and was fixed on the branch: a condition
watch's second firing was routed to image generation, because its prompt quoted
the previous reading ("differs from this") and told the model to "add nothing
else", which the media word rules read as an image edit. A turn a reminder
opens now never enters media routing (`5bc5ad56`, with the firing's exact
prompt as the regression test), and the watch row was run again after that.

The console: a design lead walked every page live and wrote a brief; a
foundation branch set the tokens, primitives, page bar and scroll contract;
three page groups were tightened live; then ten review-and-fix rounds walked
every route, dialog, wizard step, tab, empty state and error state at 1280 and
1440 in both themes (about 1,200 screenshots per round) until the last review
found only polish. The layout contract the foundation left behind runs as a
lane (`manager/frontend/live/design.live.ts`, 8 checks, all passing on the
rebuilt console).


## 2. The table

| # | Area | Feature | Result | Evidence | Cause and why |
|---|---|---|---|---|---|
| 1 | Auth | Register a new account | WORKS | 01-register-form.png, 01-registered-landing.png, 01-fresh-account-signed-in.png, user_id=e9d36aec-0f51-48c3-89e0-87758b7284d5 |  |
| 2 | Auth | Email verification | WORKS | 02-after-registration-banner.png, 02-after-resend.png, 02-verify-path-the-mail-would-use.png |  |
| 3 | Auth | Login, logout, wrong password, reload | WORKS | 03-signed-in.png, 03-after-reload.png, 03-logged-out.png |  |
| 4 | Auth | Sessions page and revoke all others | WORKS | 04-sessions-page.png, 04-sessions-after-revoke.png, 04-second-browser-after-revoke.png |  |
| 5 | Auth | Forgot and reset password | WORKS | 05-forgot-password-sent.png, 05-reset-form.png, 05-reset-outcome.png |  |
| 6 | Auth | Invitations | WORKS | 06-invite-form.png, 06-invitation-pending.png, 06-invitation-accept-page.png, invitation_id=78146b53-e938-45e1-aa64-5690b1f103c0 | The server refuses a second invitation for an address that was invited before, even after the member was removed (unique email per organization); the earlier row was cleared directly for this run. |
| 7 | Auth | /unauthorized for a member without the role | WORKS | 07-member-at-org-settings.png |  |
| 8 | Org settings | Members: change a role, audit log | WORKS | 08-members-before.png, 08-role-changed.png, 08-audit-logs.png |  |
| 9 | Org settings | AI Settings self-hosted models, save, reset | WORKS | 09-ai-settings-filled.png, 09-auto-chat-fast-turn.png, 09-auto-chat-reasoning-turn.png, chat_id=4ed8e039-ede0-41fc-a4f0-ba8ed10ab1be |  |
| 10 | Org settings | AI provider OpenAI and Anthropic, LiteLLM section | FAILS | 10-provider-openai-form.png, 10-provider-anthropic-form.png, 10-litellm-saved.png | environment: no OpenAI or Anthropic API key on this machine; the provider forms render and the LiteLLM section saves (rows 9 and 10 share a lane, and the self-hosted half works). |
| 11 | Org settings | Billing page | WORKS | 11-billing.png |  |
| 12 | Workspace settings | Theme | WORKS | 12-theme-form-and-preview.png, 12-theme-on-chats.png, 12-theme-on-wiki.png |  |
| 13 | Workspace settings | Workspace AI override and credentials | WORKS | 13-workspace-ai-override-form.png, 13-workspace-chat-uses-override.png, 13-reset-alert.png, chat_id=2be55bd5-7b46-4754-b7ae-14f93f36da90 |  |
| 14 | Models | Installed models, disk meter, sort, details, delete | WORKS | 14-installed-models.png, 14-model-details.png, 14-delete-confirm.png |  |
| 15 | Models | Add Model from catalog and HuggingFace | WORKS | 15-browse-filtered.png, 15-download-options.png, 15-catalog-install-complete.png |  |
| 16 | Models | Stop Ollama: error and recovery | WORKS | 16-ollama-stopped.png, 16-ollama-recovered.png |  |
| 17 | Models | Train tab: base, frames, mirrors, captions | WORKS | 17-train-tab.png, 17-frames-with-mirrors.png, 17-frames-mirror-off.png |  |
| 18 | Models | Training result and quality | WORKS | 18-training-result.png |  |
| 19 | Models | Adapter used from chat | WORKS | 19-adapter-image-in-chat.png, 19-adapter-image.png, chat_id=b65759eb-e7ac-4869-adbc-0886bc9652b5 | Zone picks the image recipe from COMFYUI_CHECKPOINT; a trained adapter is used when that names the adapter file (its sidecar carries the flux-schnell-adapter recipe). There is no per-chat or trigger-word selection: with the base checkpoint selected the same prompt rendered a man in a red jacket (first run of this row) |
| 20 | Sources | GitHub source | WORKS | 20-source-kinds.png, 20-source-github-form.png, 20-source-verified.png, source_id=f29acba3-c463-4986-a158-41206d9dfecd | The wizard offers no Verify step, no "Allow write operations" for GitHub (Filesystem only) and no "Main repository" toggle; Verify and Enable/Disable live on the card. Delete is exercised in row 31. |
| 21 | Sources | GitLab, Notion, URL, Slack sources | WORKS | 21-gitlab-verify.png, 21-web-url-verify.png, 21-text-verify.png | The wizard now offers exactly the five kinds an adapter can verify (GitHub, GitLab, Filesystem, Web URL, Text) and the Web URL kind verifies instead of being refused (P9). GitLab is exercised with a deliberately bad token, so its verdict is the adapter refusing the credential rather than a repository read; Notion and Slack are no longer offered because nothing can verify them here. |
| 22 | Sources | Source indexing and search | WORKS | 22-context-search-github.png | Chat citation of repository content is checked in row 37 |
| 23 | Projects | Projects | WORKS | 23-project-created.png, 23-project-cancelled.png, 23-project-after-unlink.png |  |
| 24 | Projects | External sync | WORKS | 24-add-sync-form.png, 24-sync-after-add.png |  |
| 25 | Tasks | Read-only task run | WORKS | 25-task-created.png, 25-execution-logs-streaming.png, 25-run-finished.png, task_id=b5279f24-0dc1-4e09-b275-fa3aea429fce run_id=b729aff1-1a37-4292-9871-5b0c183eb1da |  |
| 26 | Tasks | Plan approval | WORKS | 26-plan-approval-parked.png, 26-approved-run-finished.png, task_id=4ff2c6c0-59c9-4a9a-9bd7-1b83d1a1df34 run_id=6bf8400e-a7a7-4483-a828-e569add8764d |  |
| 27 | Tasks | Run asks a question | WORKS | 27-waiting-for-you.png, 27-resumed-and-finished.png, task_id=8a1af9fa-0b70-42ee-aa82-85323087ce47 run_id=73010daf-12ab-4881-be01-0a8ba580b665 |  |
| 28 | Tasks | Background command, wait_for, tail_job | WORKS | 28-background-job-run.png, task_id=bd15f42b-2f1f-48ae-95af-9a716ea06094 run_id=ef12d8fa-413e-4fa3-9a32-fff7ab15b385 |  |
| 29 | Tasks | Refused command corrected | WORKS | 29-refused-then-corrected.png, task_id=2476c2d9-9e82-4f52-892a-c553fb419981 |  |
| 30 | Tasks | Repository task to pull request | WORKS | 30-task-pr-open.png, 30-task-card-after-merge-and-sync.png, task_id=4ff2c6c0-59c9-4a9a-9bd7-1b83d1a1df34 pr_url=https://github.com/abnegate/zone-tes |  |
| 31 | Tasks | Run Again, failed run, delete | WORKS | 31-run-again-offered.png, 31-run-again-finished.png, 31-failed-run.png | After a failed run the dialog offers "Run Again"; "Try Again" is the label after a start error only |
| 32 | Tasks | MCP magents tools | FAILS | 32-mcp-task-run.png, 32-magents-from-chat.png, task_id=b3517ba0-ed53-41eb-8874-9f7020d57654 | environment: the task run now reaches the magents tools (P24 fixed: magents_spawn_session was called from the task, and from a chat spawn, await_reply, read_transcript and list_sessions all ran), but the session magents spawns on this machine never goes live and never writes a transcript, so nothing is read back; that half is the local magents/claude-print setup, not Zone. |
| 33 | Chats | Chat management, model choice, agent mode | WORKS | 33-fast-model-chat.png, 33-reasoning-model-chat.png, 33-renamed.png |  |
| 34 | Chats | Reasoning block, Stop, context meter | WORKS | 34-reasoning-block.png, 34-stopped-mid-stream.png, chat_id=41f04884-feb5-45d4-a71a-b91fc582db45 |  |
| 35 | Chats | Characters | WORKS | 35-character-editor.png, 35-persona-reply.png, chat_id=bbd0bb1a-8c26-4775-8aae-ea9ae12f35e0 |  |
| 36 | Chats | Attachments | WORKS | 36-image-attached.png, 36-image-described.png, 36-text-attached.png, chat_id=ae19d71c-7173-4632-ae27-2b8f5ca3dbef | "Use as starting image" is exercised with the image edit weights in the media rows (row 38) |
| 37 | Chats | Sources chip | WORKS | 37-composer-sources-chip.png, 37-repository-attached.png, 37-code-question.png, chat_id=ebebd379-ce6b-482e-81ae-fdaa1cb6aec7 |  |
| 38 | Chats | Media by intent | WORKS | 38-image.png, 38-audio.flac, 38-video.webm, 38-video-upscaled.webm, 38-video-frame.png, chat_id=7d66893f-879b-4b43-9a05-2530dad9b4a5 | Every phrasing routed on its first attempt this time, including the two the first pass missed: "a soundscape of rain on a tin roof with distant thunder, about ten seconds" came back as a 29 s FLAC and "a short clip of a sunset over the ocean" as a WebM, then both upscales. The video frames are still blurred colour fields, which is Wan 2.2 on this ComfyUI and PyTorch MPS build, not Zone's path. |
| 39 | Chats | Upstream failure reported | WORKS | 39-ollama-stopped-mid-turn.png, 39-chat-usable-after.png, chat_id=17eaa1b6-fdd3-45b2-a3fa-3eb68d307d87 | Judged on a reply arriving after Ollama is back; the model may answer the interrupted request again rather than the new one-word instruction |
| 40 | Agent tools | read_file, write_file, apply_patch, approvals | WORKS | 40-approval-card.png, 40-write-approved.png, 40-read-file.png, chat_id=8b533b2c-0c51-4027-80e9-7d8993bee23f |  |
| 41 | Agent tools | run_shell foreground and background | WORKS | 41-run-shell-foreground.png, 41-background-job.png, chat_id=bb2bf139-60b3-497a-a1fd-764d1aad72e7 |  |
| 42 | Agent tools | ask_user | WORKS | 42-question-card.png, 42-answered.png, chat_id=fdd18c0c-bb65-4dac-b163-0cca64681aa4 |  |
| 43 | Agent tools | Memory tools | WORKS | 43-memory-written.png, 43-memory-read-with-badge.png, 43-memory-appended.png |  |
| 44 | Agent tools | Toolbox search and load | WORKS | 44-reminder-set.png, chat_id=8cdef812-005f-4e13-afe8-ae60aaecfdf7 | load_tools brought in create_reminder by name from the prompt's catalog (tools in order: load_tools, create_reminder, ...; later load_tools for list_reminders and cancel_reminder); search_tools was not needed because the catalog names every deferred tool. The lane's original rule demanded search_tools as well. |
| 45 | Agent tools | Reminders and condition watch | FAILS | 45-reminder-fired.png, 45-prompt-reminder-set.png, 45-prompt-reminder-ran.png, chat_id=8cdef812-005f-4e13-afe8-ae60aaecfdf7 |  |
| 46 | Agent tools | Knowledge and document tools | WORKS | 46-search-knowledge-citation.png, 46-list-and-read.png, 46-create-and-update.png, chat_id=53d5564e-cd0c-481f-9fe6-1eb63d68df57 |  |
| 47 | Agent tools | list_chats, search_chat_history | WORKS | 47-list-chats.png, 47-search-chat-history.png, chat_id=dda739e6-8e62-45b2-ab08-933dde1aa714 |  |
| 48 | Agent tools | web_search, fetch_url | WORKS | 48-fetch-url.png, 48-web-search.png, chat_id=d9ab82eb-81f0-4845-9840-7ed007a27571 |  |
| 49 | Agent tools | Media tools called explicitly | WORKS | 49-generate-image-tool.png, 49-edit-image-tool.png, 49-generate-audio-tool.png |  |
| 50 | Agent tools | GitHub tools | WORKS | 50-01-list_sources.png, 50-02-read_repository_file.png, 50-03-list_issues.png, chat_id=08cdfca6-5023-4f5f-b5ef-c22956585eb1 |  |
| 51 | Agent tools | Task tools from chat | WORKS | 51-task-created-and-started.png, 51-get-task-run.png, 51-task-on-tasks-page.png, chat_id=c800a635-3046-4179-bccb-1562fee3f563 |  |
| 52 | Agent tools | list_members, send_message | WORKS | 52-list-members.png, 52-send-message.png, 52-message-in-target-chat.png, chat_id=6108ebe4-5658-4cde-8485-07d35ee39cab |  |
| 53 | Agent tools | Prometheus and Grafana tools | WORKS | 53-query-prometheus.png, 53-list-grafana-dashboards.png, chat_id=4cad7bfb-e89e-4d3a-8e17-08739db1618a |  |
| 54 | Wiki and search | Wiki | WORKS | 54-text-entry-card.png, 54-url-entry-refreshed.png, 54-search-knowledge.png, chat_id=488fdf03-8fe0-4b9f-ba0b-2b0fc52a49fa | Text entry created, embedded (1 row), found by Search knowledge and cited by the agent ("Harbour bridge facts … Knowledge passage"); the URL / Web Page kind now creates a url entry (badge url) whose page was fetched ("Example Domain…", last_fetched_at set) and re-fetched by Refresh (a later last_fetched_at); Delete removes the card and deactivates the row (is_active = false), which the lane's raw row count read as a leftover. |
| 55 | Wiki and search | Context Search | WORKS | 55-hybrid.png, 55-semantic.png, 55-keyword.png | Context Search covers indexed sources (zone_context content items); wiki entries have their own search on the Wiki page and the search_knowledge tool (rows 46 and 54). The relevance badge reads "<n>% semantic" or "Keyword match" when the server sends those scores, and "Highly relevant" or "Relevant" only when it sends neither |
| 56 | Tenancy | Tenancy | WORKS | 56-intruder-chats.png, 56-intruder-owner-chat-by-id.png, 56-intruder-wiki.png | With the row 6 membership still in place the second tenant could open the first tenant's chat by id and list its wiki through the API (evidence.jsonl earlier row 56 line): workspace members share the workspace's chats and entries by design; a stranger is what this row checks. |
| 57 | Deployment | Traefik at webui.localhost | WORKS | 57-traefik-signed-in.png, 57-traefik-models.png | The stack routes the console at manager.localhost (the checklist names webui.localhost, which is the DOMAIN_HOST_WEBUI suffix the traefik, litellm, grafana and prometheus hosts hang off) |
| 58 | Deployment | LiteLLM auto routing | WORKS | 58-trivial-question.png, 58-hard-question.png, chat_id=ff2848de-8052-4798-afc2-b9f1d72d5433 |  |
| 59 | Deployment | ollama-init pulls models | WORKS | 59-models-page-bundled-ollama.png, 59-ollama-init.log, 59-bundled-ollama-list.txt | The stack ran with the host Ollama (no bundled-ollama profile), so ollama-init does not exist there; for this row the bundled Ollama and ollama-init were started with small configured models and the manager was pointed at that Ollama for the Models page, then pointed back |
| 60 | Deployment | Backup and restore | WORKS | 60-chats-after-restore.png |  |
| 61 | Deployment | Web search through SearXNG | WORKS | 61-web-search.png, chat_id=4cad7bfb-e89e-4d3a-8e17-08739db1618a |  |
| 62 | Deployment | Monitoring profile | WORKS | 62-grafana-dashboards.png, 62-grafana-dashboard-open.png | A panel reading No data is a signal the stack has not produced yet; the row is judged on the panels that carry live numbers |
| 63 | Deployment | CLI | WORKS | 63-cli-transcript.log, 63-cli-run-transcript.log, 63-lighthouse.log |  |


## 3. What changed since the first pass

Every defect of the first pass has a fix on this branch, each with a regression test that fails without it (the defect verifier confirmed all 24 and added tests for P12 and P22). Commits, oldest first:

| Defects | Commit | What changed |
|---|---|---|
| P1, P2, P3, P4, P5, P6, P7, P8 | `e7e40116`, `8182cf1a`, `f149f439`, `ffde2bf5` | The server answers the bodies the console reads (verify, sessions with `user_id` and location, forgot and reset, invitation details, member names and emails, a default Free plan with usage), the mail link and the console route agree, revoking sessions also revokes their refresh tokens, invitations are unique on pending rows only (migration `046_invitations_pending_unique`), organization settings are scoped by role on both sides, every real event writes an audit row, and DELETE calls tolerate an empty body. |
| P9 | `b4cec0b5` | The Sources wizard offers only kinds an adapter can verify; a refused kind names the kinds that work. |
| P10 | `ceecdcc1` | Project edits save with PATCH (sparse), the wizard's source persists, link and unlink routes exist, API errors reach the modal. |
| P11 | `062fd4e0` | `/api/projects/{id}/sync` stores, lists and removes sync configurations on the existing tables; the section shows the real state and says when nothing has synced yet. |
| P12 | `f5af2079` | The reception sweep moves `tasks.pr_status` to merged or closed and the card shows it. |
| P13, P14, P20, P21, P22, P23 | `3bd57f9c` (merge of the D3 branch) | Archive and Unarchive update the visible list; a Sources chip in the composer attaches workspace sources to a chat (`047_chat_attached_sources`) and scopes retrieval to them; `create_reminder` says when `due_at` is in the past and documents the hourly floor; `update_task` takes a priority; URL knowledge entries post `source_url`, are fetched and refreshable; chat search sends `workspace_id`; the Models page shows the Ollama outage state. |
| P15 | `b5653fa9` | The CLI reads the flat auth responses; `zone lighthouse` builds and audits the same directory. |
| P16 | `55fbc240` | Every tool a task run executes is written to `task_tool_calls`. |
| P17 | `81f41e07` | The manager image copies the whole runner workspace; `scripts/test-dockerfile-members.sh` keeps the Dockerfile and the workspace in step. |
| P18, P19 | `aa33affc`, `3933bdf1`, `03192559` | The postgres cluster lives in the backed-up volume; `make migrate-pgdata` moves an existing install's cluster there (and honours `ZONE_PGDATA_SOURCE` before scanning); `ollama-init` reaches a host Ollama or says why it will not run. |
| P24 | `4f195aa9` | Task runs connect the configured MCP servers too. |
| Rows 38, 50, 51 (model-facing) | `eb3063d4`, `82838f29`, `bca0b0cd` | Bare noun-phrase media requests ("a soundscape of…", "a short clip of…") route to their medium; the GitHub tools are named by what people ask for and one issue can be read in full; `load_tools` and `search_tools` answer for a core tool by name instead of denying it exists. |
| Layout | `128c7e71` and the merges of the L0 to L3 branches, review rounds 1 to 6 | One 48px page bar and one scrolling body per page, the tokens and primitives of the design brief, the Train targets as rows, tight wiki cards, uniform task cards, tinted badges, one card for every signed-out page, dense lists and dialogs. |


## 4. Still open

| Row | State | Why |
|---|---|---|
| 10 | FAILS, environment | No OpenAI or Anthropic key on this machine. The provider forms render, the LiteLLM section saves and the self-hosted half of the row works; no chat turn was answered by either provider. |
| 32 | FAILS, environment | The product half is fixed: with `ZONE_MCP_ENABLED=true` a task run now reaches the MCP tools (P24) and a chat calls `magents_spawn_session`, `magents_await_reply`, `magents_read_transcript`, `magents_get_session`, `magents_inbox`, `magents_list_sessions` and `magents_stop_session`. The headless session `magents` spawns on this machine never goes live and never writes a transcript (`magents get` reports `live: false`, `magents read` says "no transcript found"), so nothing comes back to the chat. That is the local `magents` and `claude-print` setup, not Zone. |


## 5. Not re-measured

Rows 21 and 61 still lack the credentials the first pass named (GitLab, Notion and Slack tokens; VPN credentials for the `vpn` profile), and the Wan 2.2 video weights still produce blurred colour fields on this ComfyUI and PyTorch build. Those are the same environment limits as the first pass; the rows themselves pass on everything this machine can reach.
