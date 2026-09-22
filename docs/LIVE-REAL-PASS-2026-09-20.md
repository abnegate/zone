# Live real pass, 2026-09-20

Every feature in the checklist, driven through the console against a real
`zone-server` on real Ollama weights, a real native ComfyUI with every bundle,
real Postgres and Valkey-compatible Redis, and the real scratch repository on
GitHub. No stand-in and no mock ran at any point: `ZONE_LIVE_MODEL_STUB` stayed
unset, neither stub script was started, and the trainer was ComfyUI's own.

Evidence for every row -- the screenshots named in the table,
`evidence.jsonl` (one JSON line per row with the ids, database rows, tool calls
and log lines each lane read), the media the chat produced, and the training
clip -- was kept in `docs/live-real-pass-2026-09-20/` and removed from the tree
at d1a6ac8e, rather than carry 43 MB of it in the repository for good. `git show
d1a6ac8e:docs/live-real-pass-2026-09-20/<file>` reads any of it back while this
branch exists. The lanes that drove the rows are under
`manager/frontend/live/pass/`, opt-in with `ZONE_LIVE_REAL_PASS=1`.

## 1. The machine

| | |
|---|---|
| Hardware | Apple M4 Max, 40 cores, 64 GB unified memory, macOS 26.5.2 |
| Ollama | 0.34.0, native (Metal) |
| ComfyUI | 0.34.0, native, Python 3.13.15, PyTorch 2.9.1, MPS |
| Postgres | 18.6 (Homebrew) with pgvector 0.8.6 |
| Redis | 8.10.1 (Homebrew), a dedicated instance on port 6390 for the rig |
| Chat and tools model | `qwen3.8:27b` (Qwen 3.5 family, 27.3B, Q4_K_M, 17.7 GB) for both `OLLAMA_MODEL_FAST` and `OLLAMA_MODEL_REASON` |
| Tool test | 3 of 3 `/v1/chat/completions` calls with one tool carried `tool_calls` |
| Embeddings | `qwen3-embedding:0.6b` (1024 dimensions) |
| Vision | `llava:7b` |
| Prompt evaluation | 146 tokens/s cold, 648 tokens/s warm |
| Generation | 12.0 to 12.7 tokens/s |

Direct renders against ComfyUI's own API with the workflows in
`comfyui/workflows/`, before the pass, all kept under the scratch directory and
summarised here:

| Render | Workflow | Time | Result |
|---|---|---|---|
| Image 1024×1024 | `flux1-schnell-fp8-api.json` | 123 s | A lighthouse on a stormy coast, as prompted |
| Upscale ×4 | `upscale-image-api.json` | under 30 s | 4096×4096 PNG |
| Audio 10 s | `ace-step-v1-3.5b-api.json` | 16 s | FLAC, 9.94 s |
| Video 832×480, 49 frames | `wan2.2-ti2v-5b-api.json` | 1199 s | A WebM of the right length whose frames are blurred colour fields, not the prompted scene (see `17-clip-extension-attempt-unusable.png` for the same failure on image-to-video, and the note under row 38) |
| Image-to-video 640×640, 49 frames, three runs | `wan2.2-ti2v-5b-i2v-api.json` | 466 s, 480 s, 476 s | The start frame decays into noise within a few frames |

The Wan 2.2 5B renders are unusable on this ComfyUI build, and were already so
in this machine's earlier passes (the `zone-video_0000{1,3}_.webm` outputs
ComfyUI kept from 6 and 11 September show the same blur). That is a property of
the weights on this runtime, not of Zone: Zone's video path submits, waits,
collects and plays the clip correctly. It is classified `environment` where it
matters.

What the chat produced through Zone's own path (intent, ComfyUI job, poll,
collect, play), timed from the message to the attachment:

| Through the chat | Time | Result |
|---|---|---|
| Image 1024×1024 (row 38) | 278 s | `38-image.png` |
| Upscale ×4 of that image | 18 s | 4096×4096 |
| Audio (row 38) | 116 s | `38-audio.flac`, FLAC |
| Video 832×480, 24 fps, 2 s (row 38, `real-media` lane) | 671 s | `38-video.webm`, blurred colour fields (environment, see above) |
| Upscale ×4 of that video | 202 s | `38-video-upscaled.webm`, 3328×1920 |
| Image edit (row 36) | see `evidence.jsonl` row 36.5 | `36-edit-original.png`, `36-edit-result.png` |
| LoRA training on the 4 s clip (row 18, Train tab) | 29 min from Train to the result (150 steps at about 6 s each, then four probe renders); the `real-train` lane's run earlier in the day took 21 min alone, and an attempt with the 27B chat model resident ended in a NaN loss | adapter `live-pass-zrkxyz-1789892614987.safetensors`, "Strong, 49% better", 3 near-duplicate frames set aside (`18-training-result.png`); the lane run: `18-real-train-lane.log`, `live-zrkxyz-1789875244662.safetensors`, 47% better |

Everything else timed is in `evidence.jsonl`.

Two other `zone-server` processes from earlier sessions on this machine (ports
8010 and 8020, started on 13 and 14 September, pointed at the same Ollama and
ComfyUI) were left running untouched; their logs show no activity during the
pass. The compose stack of section 4 ran with the host Ollama and this ComfyUI.

The rig is `scripts/live-verify.sh` with `ZONE_LIVE_KEEP=1`, its server
restarted by hand twice with extra configuration and nothing else changed:
once with `ZONE_MCP_ENABLED=true`, `SEARCH_ENABLE_WEB_SEARCH=true` and
`SEARCH_SEARXNG_QUERY_URL` pointed at a SearXNG container on 127.0.0.1:8089
(rows 32 and 48) plus `ZONE_CHAT_AGENT_CWD` so chat tools write under the rig
instead of the checkout, and once more with
`COMFYUI_CHECKPOINT=live-pass-zrkxyz-1789892614987.safetensors` so the adapter
row 18 trained becomes the image model row 19 renders with.

## 2. The table

| # | Area | Feature | Result | Evidence | Cause and why |
|---|---|---|---|---|---|
| 1 | Auth | Register a new account | WORKS | 01-register-form.png, 01-registered-landing.png, 01-fresh-account-signed-in.png, user_id=7479611d-9979-4f9b-b4e2-e132720518b3 |  |
| 2 | Auth | Email verification | FAILS | 02-after-registration-banner.png, 02-after-resend.png, 02-verify-path-the-mail-would-use.png | product: the server verified the address and answered {"message":"Email verified successfully"} but the console shows Verification Failed because its schema expects a boolean `success` |
| 3 | Auth | Login, logout, wrong password, reload | WORKS | 03-signed-in.png, 03-after-reload.png, 03-logged-out.png |  |
| 4 | Auth | Sessions page and revoke all others | FAILS | 04-sessions-page.png, 04-sessions-after-revoke.png, 04-second-browser-after-revoke.png | product: /sessions renders "No active sessions found" with a validation error, because the console schema requires user_id and location that GET /api/auth/sessions does not send |
| 5 | Auth | Forgot and reset password | FAILS | 05-forgot-password-sent.png, 05-reset-form.png, 05-reset-outcome.png | product: the console reports a failure where the server succeeded (forgot: "Zone Reset your password Email Validation failed: success: Invalid input: expected boolean, received undefined Send Reset Link Remember your password? Sign in"; reset: "Zone Set new password New Password Confirm Password Validation failed: success: Invalid input: expected boolean, received undefined Reset Password Back to Login") |
| 6 | Auth | Invitations | FAILS | 06-invite-form.png, 06-invitation-pending.png, 06-invitation-accept-page.png, invitation_id=ff853077-9070-4e23-8ee0-4b87ab1f5d9d | product: /invitations?token= shows "Invalid Invitation" (console schema wants workspace_name, invited_by_email and an ISO expires_at that GET /api/invitations/{token} does not send); accepted through the API instead; the Members table then lists the member only as "Member —" because the members API sends user ids without names or emails |
| 7 | Auth | /unauthorized for a member without the role | FAILS | 07-member-at-org-settings.png | product: a member of the organization opens /org-settings and sees its settings; only the API refuses the writes |
| 8 | Org settings | Members: change a role, audit log | FAILS | 08-members-before.png, 08-role-changed.png, 08-audit-logs.png | product: the role change itself works (Member → Admin saved and shown, 08-role-changed.png), but Audit Logs stays "No audit logs found." and the audit_logs table has 0 rows after the whole pass (P6, log_action has no callers); the Members table lists every member as "Member —" because the members API sends user ids only (P4). |
| 9 | Org settings | AI Settings self-hosted models, save, reset | WORKS | 09-ai-settings-filled.png, 09-auto-chat-fast-turn.png, 09-auto-chat-reasoning-turn.png, chat_id=72b6e9e6-f0fa-480d-8940-224e501f0bad |  |
| 10 | Org settings | AI provider OpenAI and Anthropic, LiteLLM section | FAILS | 10-provider-openai-form.png, 10-provider-anthropic-form.png, 10-litellm-saved.png | environment: no OpenAI or Anthropic API key on this machine; the provider forms render and the LiteLLM section saves |
| 11 | Org settings | Billing page | FAILS | 11-billing.png | product: Billing shows "No subscription found for this organization" and a Retry button for an organization without a subscription row; no usage counts are shown (P8). |
| 12 | Workspace settings | Theme | WORKS | 12-theme-form-and-preview.png, 12-theme-on-chats.png, 12-theme-on-wiki.png |  |
| 13 | Workspace settings | Workspace AI override and credentials | WORKS | 13-workspace-ai-override-form.png, 13-workspace-chat-uses-override.png, 13-reset-alert.png, chat_id=14d99a5b-fce7-43f1-908d-e7a612ba4329 |  |
| 14 | Models | Installed models, disk meter, sort, details, delete | WORKS | 14-installed-models.png, 14-model-details.png, 14-delete-confirm.png | 15 installed rows with size, parameters, quantisation and date ("1.2 GB · 1.2B · Q8_0 · 9/20/2026"), capability chips (Text, Tools), the disk meter at 84% ("779.5 GB used of 926.4 GB · 97.5 GB in models"), the details modal ("SIZE 16.5 GB PARAMETERS 27.3B QUANTIZATION Q4_K_M FAMILY qwen35"), and Delete removed qwen2.5:0.5b from Ollama's own tag list; sort lives on the Browse tab only. The lane mis-scored the modal by looking for "Size" in lower case. |
| 15 | Models | Add Model from catalog and HuggingFace | WORKS | 15-browse-filtered.png, 15-download-options.png, 15-catalog-install-complete.png |  |
| 16 | Models | Stop Ollama: error and recovery | FAILS | 16-ollama-stopped.png, 16-ollama-recovered.png | product: with Ollama stopped (its /api/tags refused), Refresh on the Models page showed no error state; GET /api/models answered 200 [{"name":"ace_step_v1_3.5b.safetensors","completion":false,"size":7699743341,"modified_at":"2026-09-09T11:11:10.0776 and the page kept 8 rows |
| 17 | Models | Train tab: base, frames, mirrors, captions | WORKS | 17-train-tab.png, 17-frames-with-mirrors.png, 17-frames-mirror-off.png |  |
| 18 | Models | Training result and quality | WORKS | 18-training-result.png | Train from the Train tab: 29 minutes from the click to "Training finished: live-pass-zrkxyz-1789892614987.safetensors, Strong, 49% better measured at final" (150 steps at about 6 s each, then four probe renders); screening set aside 3 near-duplicate frames and trained on 5 of 8, "Your originals are untouched". The adapter file and its sidecar landed in the ComfyUI loras directory. See row 19 for how weak an adapter this small clip gives in practice. |
| 19 | Models | Adapter used from chat | WORKS | 19-adapter-image-in-chat.png, 19-adapter-image.png, 19-base-checkpoint-image.png, chat_id=48197f89-b7ec-43c6-88e2-5d4995d586dc | With COMFYUI_CHECKPOINT naming the adapter, the chat render for "draw me zrkxyz standing in a snowy pine forest" carried LoraLoaderModelOnly: live-pass-zrkxyz-1789892614987.safetensors (read from ComfyUI's queue while it ran) and came back as an image in 11 minutes behind an orphaned training job. Resemblance is poor: the adapter changed the style and wrote a zrkxyz-like scrawl in the corner, but the subject is still a person, not the robot teapot of the clip (19-adapter-image.png against 19-base-checkpoint-image.png from the base checkpoint). Eight frames, five after screening, 150 steps: the adapter is real but weak, and the "Strong, 49% better" probe score of row 18 overstates what a render shows. There is no per-chat or trigger-word adapter selection: the first run with the base checkpoint selected ignored the trigger word. |
| 20 | Sources | GitHub source | WORKS | 20-source-kinds.png, 20-source-github-form.png, 20-source-verified.png, source_id=b004c4b9-52e0-4808-96c5-76f3ce996618 | The wizard offers no Verify step, no "Allow write operations" for GitHub (Filesystem only) and no "Main repository" toggle; Verify and Enable/Disable live on the card. Delete is exercised in row 31. |
| 21 | Sources | GitLab, Notion, URL, Slack sources | FAILS | 21-gitlab-verify.png, 21-web-url-refused.png, 21-text-verify.png | environment: no GitLab, Notion or Slack token on this machine; the GitLab wizard refuses a bad token on Verify as recorded, Notion and Slack have no wizard entry, and the Web URL kind is refused by the server |
| 22 | Sources | Source indexing and search | WORKS | 22-context-search-github.png | Chat citation of repository content is checked in row 37 |
| 23 | Projects | Projects | FAILS | 23-project-created.png, 23-project-cancelled.png, 23-project-after-unlink.png | product: Unlink calls no Unlink button; the server registers no /api/projects/{id}/source route and the console shows no error |
| 24 | Projects | External sync | FAILS | 24-add-sync-form.png, 24-sync-after-add.png | product: + Add Sync posts to /api/projects/{id}/sync and the server answers 404; nothing is synced |
| 25 | Tasks | Read-only task run | WORKS | 25-task-created.png, 25-execution-logs-streaming.png, 25-run-finished.png, task_id=122cbaed-1350-43f4-9d94-7204a8e38ae2 run_id=51683a18-40b9-4a02-abfe-8a82defb17c5 |  |
| 26 | Tasks | Plan approval | WORKS | 26-plan-approval-parked.png, 26-approved-run-finished.png, task_id=9ce5cf7b-21b4-42db-8d0e-094bb09a6c98 run_id=d86b77d0-6390-45c1-a375-7ba4e9f4d269 |  |
| 27 | Tasks | Run asks a question | WORKS | 27-waiting-for-you.png, 27-resumed-and-finished.png, task_id=2e6357c6-d7d2-4242-8b9a-e6092e4c0c20 run_id=3e1a2904-7694-4847-a84a-38fe05fdd3c4 |  |
| 28 | Tasks | Background command, wait_for, tail_job | WORKS | 28-background-job-run.png, task_id=72362ece-3ccb-48e8-bccb-69311ed0c7cf run_id=702bbed3-4e0e-4c8c-969f-8a0c3e38658c |  |
| 29 | Tasks | Refused command corrected | WORKS | 29-refused-then-corrected.png, task_id=56e0a341-4b70-4f86-ae42-0adb239e2193 |  |
| 30 | Tasks | Repository task to pull request | FAILS | 30-task-pr-open.png, 30-task-card-after-merge-and-sync.png, task_id=9ce5cf7b-21b4-42db-8d0e-094bb09a6c98 pr_url=https://github.com/abnegate/zone-tes | product: the run opened and pushed the pull request and the card showed "PR: open" and "View Pull Request"; after the merge the reception sweep recorded merged_at on the run, but nothing writes tasks.pr_status, so the card never reads "PR: merged" |
| 31 | Tasks | Run Again, failed run, delete | WORKS | 31-run-again-offered.png, 31-run-again-finished.png, 31-failed-run.png | After a failed run the dialog offers "Run Again"; "Try Again" is the label after a start error only |
| 32 | Tasks | MCP magents tools | FAILS | 32-mcp-task-run.png, 32-magents-from-chat.png, task_id=e04bda50-b46d-4480-a49b-2e95f8df7636, chat_id=50766993-01c9-46ae-854b-c24dc46d1003, task_id=e04bda50-b46d-4480-a49b-2e95f8df7636 | product and environment. From a chat the MCP side works: with ZONE_MCP_ENABLED=true the server connected magents (22 tools, "Attached MCP tools to chat tools=22") and the model called magents_spawn_session, await_reply, read_transcript, get_session, inbox, list_sessions, session_digest and stop_session, each result shown in the console wrapped as untrusted MCP output (evidence row 32.5, 32-magents-from-chat.png). Environment: the session magents spawned (claude, origin claude-print) never went live and never wrote a transcript (magents get: live=false, "no transcript found"), so nothing came back to the chat. Product: a task run never gets MCP tools at all, because for_task assembles its tools with connect=false (P24); the task of the first attempt searched its catalog six times and never saw a magents_* tool. Also worth knowing: with Auto-approve on, the chat agent then ran run_shell to list and read files under ~/.claude-personal/projects while chasing the transcript. |
| 33 | Chats | Chat management, model choice, agent mode | FAILS | 33-fast-model-chat.png, 33-after-archive-click.png, 33-search-results.png, 33-agent-off-no-tools.png, 33-agent-on-tools.png | product: two defects in one page. Archive and Unarchive change the chat on the server (chats.archived flips at once) but the list being viewed keeps showing it until a reload (P13); and the chat search never shows results because the console calls /api/chats/search without workspace_id and the server answers 400 (P22). The rest works: a fast-model chat and a reasoning-model chat answered, rename, archive, unarchive and delete (0 rows after), and agent mode gates the tools (no tool call with Agent off for "run uname -a", run_shell with it on). |
| 34 | Chats | Reasoning block, Stop, context meter | WORKS | 34-reasoning-block.png, 34-stopped-mid-stream.png, chat_id=fe88f5d9-82c4-44de-b2e0-6167bcd346bd |  |
| 35 | Chats | Characters | WORKS | 35-character-editor.png, 35-persona-reply.png, chat_id=8cb86026-ebe8-410a-9356-48d67a26b8a2 |  |
| 36 | Chats | Attachments | WORKS | 36-image-attached.png, 36-image-described.png, 36-text-attached.png, chat_id=940afa1a-ae71-45ea-a40a-347bd751d802 | "Use as starting image" is exercised with the image edit weights in the media rows (row 38) |
| 37 | Chats | Sources chip | FAILS | 37-composer-no-sources-chip.png, 37-code-question.png, chat_id=2b969346-a46f-40e7-bb09-cfe19f522cf6 | product: the composer has no Sources chip and no way to attach a repository to a chat; the only "Sources" element is the read-only citations block on a reply |
| 38 | Chats | Media by intent | FAILS | 38-image-rendered.png, 38-image-upscaled.png, 38-audio-rendered.png, chat_id=995863d7-f3ed-4566-a05c-6252dd42c018 | model: the intent router (COMFYUI_CLASSIFIER_MODEL, the same 27B) missed two of six plain requests: "a soundscape of rain on a tin roof with distant thunder, about ten seconds" and "a short clip of a sunset over the ocean" were answered "I can't generate audio" / "I can't produce a video clip". Everything it did route rendered for real: image 1024x1024 in 278 s and its x4 upscale in 18 s (38-image.png), audio from "make a background audio track that sounds like rain..." in 116 s (38-audio.flac), video from "make a video of a sunset over the ocean" in 671 s and its x4 upscale to 3328x1920 in 202 s (38-video.webm, 38-video-upscaled.webm, the real-media lane, evidence row 38.2), and "upscale" on an empty chat gave the alert "Upscaling needs an image or video". The video frames are blurred colour fields, which is the Wan 2.2 on MPS environment issue, not Zone's path. |
| 39 | Chats | Upstream failure reported | WORKS | 39-ollama-stopped-mid-turn.png, 39-chat-usable-after.png, chat_id=40b25c78-a03c-4ae9-80e3-96185d9a132d |  |
| 40 | Agent tools | read_file, write_file, apply_patch, approvals | WORKS | 40-approval-card.png, 40-write-approved.png, 40-read-file.png, chat_id=25d9be16-3c50-4df8-9847-8e2d6d3d592f | write approved on the card (server preview "Write 23 characters to notes-....txt"), read_file returned the line, apply_patch changed it to "second line" (file read from disk after the lane), the denied write left no file and the tool result read "Error: The user denied this tool call."; the lane's own file search missed the file because chat tools write into the server process's working directory (host_root, ZONE_CHAT_AGENT_CWD unset), which for this rig was the repository checkout |
| 41 | Agent tools | run_shell foreground and background | WORKS | 41-run-shell-foreground.png, 41-background-job.png, chat_id=fc8ba322-f1b9-44a8-9d26-40d66c28454a |  |
| 42 | Agent tools | ask_user | WORKS | 42-question-card.png, 42-answered.png, chat_id=af874aaf-98cf-4c39-a99a-f8a89121ee9e |  |
| 43 | Agent tools | Memory tools | WORKS | 43-memory-written.png, 43-memory-read-with-badge.png, 43-memory-appended.png |  |
| 44 | Agent tools | Toolbox search and load | WORKS | 44-reminder-set.png, chat_id=d1f713a7-a8f6-4cc1-a264-194eee1ca934 | load_tools then create_reminder for the deferred reminder tool; search_tools appears later in the same chat; the prompt's catalog names deferred tools, so the model loaded it by name |
| 45 | Agent tools | Reminders and condition watch | WORKS | 45-clock-reminder-set.png, 45-clock-reminder-fired.png, 45-watch-set.png, 45-watch-first-firing.png, 45-watch-fired-on-change.png, 45-reminder-cancelled.png, chat_id=d1f713a7-a8f6-4cc1-a264-194eee1ca934 | Exact reminder: "at 21:26 today" set at 21:16 fired at 21:26:57 as a new assistant message "hydrate now" (reminders row delivered, fired_count 1; evidence row 45.5, 45-clock-reminder-fired.png). Condition watch: an hourly watch (the smallest cadence the tool takes) read its file at its first firing 10 s after the due time ("contains exactly one line: v1"), the file was changed to v2 through the chat, and the next firing an hour later posted "Changed: the file now contains the single word v2 instead of v1" (evidence row 45.7, 45-watch-fired-on-change.png); list_reminders and cancel_reminder were used in the first run. The first run's failures were the model's: "two minutes from now" became 18:30 after the server refused the near-past instants with a format-sounding error (P20), and the watch it was asked for every minute cannot exist (hourly floor). |
| 46 | Agent tools | Knowledge and document tools | WORKS | 46-search-knowledge-citation.png, 46-list-and-read.png, 46-create-and-update.png, chat_id=37f8ffa5-96c9-4f30-90a6-ff4990ee6d06 |  |
| 47 | Agent tools | list_chats, search_chat_history | WORKS | 47-list-chats.png, 47-search-chat-history.png, chat_id=6aa5f3d6-8b75-469b-8bab-4b377c4d568c |  |
| 48 | Agent tools | web_search, fetch_url | WORKS | 48-fetch-url.png, 48-web-search.png, chat_id=ff708c6c-522c-41e3-8eca-a442cd548dd4 |  |
| 49 | Agent tools | Media tools called explicitly | WORKS | 49-generate-image-tool.png, 49-edit-image-tool.png, 49-generate-audio-tool.png | recorded from the database after the lane crashed reading ComfyUI's history, which was empty after a /free; screenshots were taken during the run |
| 50 | Agent tools | GitHub tools | FAILS | 50-01-list_sources.png, 50-02-read_repository_file.png, 50-03-list_issues.png, chat_id=d379430b-ac8b-415b-8ee9-34feef39b57f | model: never called github_issue, review_current, review_missing (used list_sources, load_tools, read_repository_file, list_issues, get_build_status, read_check_logs, create_pull_request, assess_pull_requests, assess_release_pipelines, list_deployments) |
| 51 | Agent tools | Task tools from chat | FAILS | 51-task-created-and-started.png, 51-start-task-named.png, 51-get-task-run-named.png, 51-tool-schemas-seen-by-model.png | model: asked in plain words to start a task (row 51: create then "Start the task ... now"; row 51b: "Start a background coding task for me now"), the model called list_projects, list_tasks, create_task and update_task, then load_tools(start_task) and search_tools, and twice answered that no tool starts a background run, while start_task's schema was in front of it the whole time (evidence row 51.3: the model itself lists start_task among the schemas it can see; it is in the core set). Told the tool's name (row 51d, evidence 51.6) it called start_task, an agentic task was created and its run completed, and get_task_run read the run back. Also: update_task has no priority field, so "set its priority to 5" cannot be honoured from chat; create_task makes a manual task that nothing in chat can start (by design: start_task creates and starts its own). |
| 52 | Agent tools | list_members, send_message | WORKS | 52-list-members.png, 52-send-message.png, 52-message-in-target-chat.png, chat_id=b1856cee-4df9-42d7-8cce-5975085d6c1b |  |
| 53 | Agent tools | Prometheus and Grafana tools | WORKS | 53-query-prometheus.png, 53-list-grafana-dashboards.png, chat_id=6d6d783b-6ec0-4c41-bd2a-79e699af9dc1 |  |
| 54 | Wiki and search | Wiki | FAILS | 54-text-entry-card.png, 54-url-entry-refreshed.png, 54-search-knowledge.png, 54-agent-cites-entry.png, 54-url-entry-deleted.png, chat_id=b6e1cc62-ef7b-4f63-8c59-74ea00270761 | product: the URL / Web Page kind is saved as a text entry whose content is the literal URL (badge "text", source_url empty, never fetched, no Refresh button), see P21. The text half works: the entry is embedded (1 row in knowledge_embeddings), "Search knowledge" finds it, the agent answers the bridge question from the workspace knowledge base and cites it ("Harbour bridge facts … Knowledge passage" citation), and Delete removes the card (the row stays as is_active = false). |
| 55 | Wiki and search | Context Search | WORKS | 55-hybrid.png, 55-semantic.png, 55-keyword.png | Context Search covers indexed sources (zone_context content items); wiki entries have their own search on the Wiki page and the search_knowledge tool (rows 46 and 54). The relevance badge reads "<n>% semantic" or "Keyword match" when the server sends those scores, and "Highly relevant" or "Relevant" only when it sends neither |
| 56 | Tenancy | Tenancy | WORKS | 56-intruder-chats.png, 56-intruder-owner-chat-by-id.png, 56-intruder-wiki.png | With the row 6 membership still in place the second tenant could open the first tenant's chat by id and list its wiki through the API (evidence.jsonl earlier row 56 line): workspace members share the workspace's chats and entries by design; a stranger is what this row checks. |
| 57 | Deployment | Traefik at webui.localhost | WORKS | 57-traefik-signed-in.png, 57-traefik-models.png | The stack routes the console at manager.localhost (the checklist names webui.localhost, which is the DOMAIN_HOST_WEBUI suffix the traefik, litellm, grafana and prometheus hosts hang off) |
| 58 | Deployment | LiteLLM auto routing | WORKS | 58-trivial-question.png, 58-hard-question.png, chat_id=7f20a95e-033c-46f2-8fc7-8ebb643ce5da | litellm log names: trivial auto,llama3.2 then qwen3.8 |
| 59 | Deployment | ollama-init pulls models | WORKS | 59-models-page-bundled-ollama.png, 59-ollama-init.log, 59-bundled-ollama-list.txt | The stack ran with the host Ollama (no bundled-ollama profile), so ollama-init does not exist there; for this row the bundled Ollama and ollama-init were started with small configured models and the manager was pointed at that Ollama for the Models page, then pointed back |
| 60 | Deployment | Backup and restore | FAILS | 60-backup-restore.txt, 60-chats-before.json, 60-chats-after-restore.png | product: `make backup` archives the `zone_postgres_data` volume, which is mounted at /var/lib/postgresql, while the postgres image keeps PGDATA at /var/lib/postgresql/data in an anonymous volume; the archive's postgres/ directory is empty (postgres/, postgres/data/), so after `make down`, removing the volume and `make restore` the stack comes up with an empty database (0 users, 0 chats) and the owner cannot sign in (60-chats-after-restore: /login) |
| 61 | Deployment | Web search through SearXNG | WORKS | 61-web-search.png, chat_id=6d6d783b-6ec0-4c41-bd2a-79e699af9dc1 |  |
| 62 | Deployment | Monitoring profile | WORKS | 62-grafana-dashboards.png, 62-grafana-dashboard-open.png | Grafana at grafana.webui.localhost signed in as admin, listed the provisioned dashboards (Zone Alerts, Chat Quality, Console, Gluetun, LiteLLM, ...) and the opened dashboard showed live numbers (Embed P95 49.5 ms, LiteLLM TTFT P95 4.85 min, Ollama 1, ComfyUI 1, Manager RSS 329 MiB) next to panels reading No data for signals the stack had not produced yet; the lane's final assertion re-counted the list links after navigating away and mis-scored it |
| 63 | Deployment | CLI | FAILS | lanes/zone-lighthouse.log, terminal transcript in evidence.jsonl row 63 | product: `zone login` fails with "error decoding response body" because the CLI expects {success, data: {...}} while the server answers a flat login response; `zone run` and `zone resume` refuse without a login; `zone setup --check`, `zone config`, `zone sessions` and `zone logout` work; `zone lighthouse` builds the console into build/ and then aborts looking for dist/ |


## 3. Defects

### Product defects

Each entry: repro, expected, observed, the log or database line, the file believed responsible.

**P1. Email verification succeeds on the server and fails in the console (row 2).**
Repro: register, call resend verification, open `/verify-email?token=<token>`. Expected: "Email Verified". Observed: "Verification Failed", first run "Validation failed: success: Invalid input: expected boolean, received undefined", second run "Invalid or expired verification token" (the page called the endpoint twice). Database: `users.email_verified = t` afterwards; the API answers `200 {"message":"Email verified successfully"}`. Files: `manager/frontend/src/api/client.ts` (the verify response schema), `manager/frontend/src/features/auth/pages/EmailVerificationPage.tsx`. Also: the link the mail would carry is `${APP_BASE_URL}/verify?token=` (`runner/zone_server/src/routes/auth.rs:750`) and the console has no `/verify` route, only `/verify-email` (`02-verify-path-the-mail-would-use.png`).

**P2. The sessions page cannot list sessions, so nothing can be revoked from it (row 4).**
Repro: sign in twice, open `/sessions`. Expected: two rows and "Revoke All Other Sessions". Observed: "Validation failed: sessions.0.user_id: Invalid input: expected string, received undefined, sessions.0.location: ..." and "No active sessions found" (`04-sessions-page.png`); `GET /api/auth/sessions` returned 37 rows without `user_id` or `location`. Files: `manager/frontend/src/features/auth/schemas.ts` (session schema), `runner/zone_server/src/routes/sessions.rs`. Related: after `DELETE /api/auth/sessions` the second browser's access token was refused (401) and it landed on `/login`, but `POST /api/auth/refresh` with that browser's refresh token still answered 200 (evidence row 4); the revocation does not reach the refresh token.

**P3. Forgot and reset password report failure while succeeding (row 5).**
Repro: `/forgot-password`, then `/reset-password?token=`. Expected: "Check your email", then "Password Reset Successful". Observed: both pages show "Validation failed: success: Invalid input: expected boolean, received undefined" (`05-forgot-password-sent.png`, `05-reset-outcome.png`); the server created the token, the old password is refused (401) and the new one accepted (200), `password_reset_tokens.used_at` set. Files: `manager/frontend/src/api/client.ts` (forgot and reset response schemas).

**P4. The invitation page cannot render an invitation, and re-inviting a removed member is refused (row 6).**
Repro: Organization settings, Invitations, Invite Member, open `/invitations?token=` as the invitee. Expected: "You've Been Invited!" and Accept. Observed: "Invalid Invitation — Validation failed: workspace_name: Invalid input: expected string, received undefined, invited_by_email: Invalid input: expected string, received undefined, expires_at: Invalid ISO datetime" (`06-invitation-accept-page.png`); `GET /api/invitations/{token}` answers `organization_name` but neither of the other two. Accepting through `POST /api/invitations/{token}/accept` works. Files: `manager/frontend/src/features/auth/pages/InvitationAcceptPage.tsx` and its schema, `runner/zone_server/src/routes/invitations.rs`. Related: `invitations` is unique on (email, organization) including accepted rows, so after a member is removed a new invitation answers `409 Invitation already exists`; and the Members table shows every member as "Member —" because `GET /api/organizations/{id}/members` sends user ids only (`06-member-listed.png`, `08-members-before.png`).

**P5. A member of another organization can open its settings page (row 7).**
Repro: as a `member` of the first tenant's organization, open `/org-settings` with that organization current. Expected: `/unauthorized`. Observed: the page renders with the organization's AI settings (`07-member-at-org-settings.png`); `GET .../settings/ai` answered 200; the writes answered 403 (`PATCH /api/organizations/{id}` and the role change). Files: `manager/frontend/src/shared/components/ProtectedRoute.tsx` (permission check is not organization-scoped), `runner/zone_server/src/routes/ai_settings.rs` (read allowed to a member).

**P6. Audit Logs never record anything (row 8).**
Repro: change a member's role, open Audit Logs. Expected: an entry. Observed: "No audit logs found." (`08-audit-logs.png`); `audit_logs` has 0 rows after the whole pass. File: `runner/zone_server/src/db/audit.rs` (`log_action` has no callers outside tests).

**P7. "Reset to Defaults" shows an error on success (rows 9, 13).**
Repro: save AI settings (organization or workspace), click Reset to Defaults. Expected: "Settings reset to defaults". Observed: "Failed to execute 'json' on 'Response': Unexpected end of JSON input" (`09-reset-alert.png`, `13-reset-alert.png`); the row is gone from `organization_ai_settings` / `workspace_ai_settings` and the page shows the defaults after a reload. Files: `manager/frontend/src/api/*` (the delete call parses an empty body), `runner/zone_server/src/routes/ai_settings.rs` (204 with no body).

**P8. Billing shows nothing for an organization without a subscription row (row 11).**
Observed: "No subscription found for this organization" and Retry; no usage counts (`11-billing.png`). Files: `manager/frontend/src/features/settings/organization/components/BillingSection.tsx`, `runner/zone_server/src/routes/billing.rs`.

**P9. The Sources wizard offers kinds the server refuses (row 21).**
GitLab: added, Verify answers "No adapter for source type: gitlab" (`21-gitlab-verify.png`). Web URL: `400 Invalid source_type. Must be one of: github, gitlab, filesystem, notion, text` in the wizard (`21-web-url-refused.png`). Notion is accepted by the server but has no wizard entry. Files: `manager/frontend/src/features/sources/config/*.ts`, `runner/zone_server/src/routes/sources.rs` (`ALLOWED_SOURCE_TYPES`), the adapter registry (3 adapters).

**P10. Projects: edits do not save, the wizard drops the chosen source, and link/unlink have no route (row 23).**
Edit Project saves with `PATCH /api/projects/{id}`; the server registers `PUT` and answers 405; the modal stays open with only a console error (browser check, `23-project-cancelled.png` shows the status unchanged). New Project with a source selected stores `source_id = null`. Link Source / Unlink call `PUT|DELETE /api/projects/{id}/source`, which does not exist (404). No console control links a project to a repository the server may check out; `POST /api/projects/{id}/github` exists only in the API and the pass used it for the task rows. Files: `manager/frontend/src/api/projects.ts`, `manager/frontend/src/features/projects/components/CreateProjectWizard.tsx`, `runner/zone_server/src/routes/mod.rs:179-188`.

**P11. External Sync has no server side (row 24).**
"+ Add Sync" posts to `/api/projects/{id}/sync` and gets 404; every project open also 404s on `GET .../sync` (`24-sync-after-add.png`). `sync_configs` and `synced_items` stay empty. The form offers Provider (GitHub, Linear) and Direction; there is no rollout or scope choice. Files: `manager/frontend/src/features/projects/pages/ProjectsPage.tsx`, `runner/zone_server/src/routes/mod.rs` (only the webhook routes exist).

**P12. A merged pull request never shows as merged on its task (row 30).**
Repro: let a repository task open a pull request, merge it on GitHub, wait for the reception sweep. Expected: "PR: merged". Observed: the card keeps "PR: open" (`30-task-card-after-merge-and-sync.png`). Log: `2026-09-20T04:42:36 DEBUG zone_server::workers::reception: Recorded reception for run d86b77d0-...: 0 review cycle(s), 0 approval(s)`; the run's `artifacts.pr` carries `"pr_state": "closed", "merged_at": "2026-09-20T04:13:23Z"`; `tasks.pr_status` stays `open`. Files: `runner/zone_server/src/workers/reception.rs` (records the reception), `runner/zone_server/src/db/tasks.rs:1431` (the only write of `pr_status`, at creation).

**P13. Archiving a chat leaves it in the Active list until a reload (row 33).**
`chats.archived` becomes true at once; the item stays listed with its Archive button until the page reloads (`33-after-archive-click.png`). File: `manager/frontend/src/features/chats/pages/ChatsPage.tsx` (`handleArchiveChat` does not refresh or filter the list).

**P14. No way to attach a repository to a chat (row 37).**
The composer has no Sources chip; the only "Sources" element is the read-only citations block on a reply (`37-composer-no-sources-chip.png`); the `chat_sources` table is a citation registry. Files: `manager/frontend/src/features/chats/pages/ChatsPage.tsx`, `runner/zone_server/src/db/chat_sources.rs`.

**P15. The CLI cannot log in, and its Lighthouse audit looks in the wrong directory (row 63).**
`zone login http://127.0.0.1:8030` → "HTTP error: error decoding response body for url (.../api/auth/login)": `runner/zone_cli/src/auth.rs` deserialises `{success, data: {...}, error}` while the server answers `{access_token, refresh_token, expires_in, token_type, user, roles, permissions}`. `zone run` and `zone resume` therefore refuse ("Not logged in"). `zone lighthouse --target manager` builds the console into `manager/frontend/build` (as `lighthouserc.json` expects) then aborts with "Build directory not found at .../manager/frontend/dist" (`runner/zone_cli/src/lighthouse.rs`). `zone setup --check`, `zone config`, `zone sessions` and `zone logout` work.

**P16. `task_tool_calls` is never written.**
Every task run in this pass left `task_tool_calls` empty; the calls are only in `task_run_logs` metadata (`Executing tool: X` / `Tool X finished`). Not user-visible, but any reporting on that table is blind.

**P17. The compose stack does not build from `main` (rows 57 to 62).**
Repro: `make up PROFILES=monitoring` on a clean checkout. Expected: the manager image builds. Observed: `failed to solve: "/zone_notify": not found` from `manager/Dockerfile`, which copies `runner/` crates one by one and misses `runner/zone_notify`, a crate `runner/Cargo.toml` names in the workspace. The pass built the image from a copy of the Dockerfile with `COPY runner/zone_notify ./zone_notify/` added, outside the repository. File: `manager/Dockerfile`.

**P18. `make backup` archives an empty database, so `make restore` restores nothing (row 60).**
Repro: `make backup`, `make down`, `docker volume rm zone_postgres_data`, `make restore BACKUP=…`, `make up`. Expected: the tenants and chats are back. Observed: the stack comes up with an empty `manager` database (0 users, 0 chats; the owner lands on `/login`, `60-chats-after-restore.png`). The archive's `postgres/` directory holds only `postgres/data/` (`60-backup-restore.txt`): `docker-compose.yml:89` mounts `postgres_data` at `/var/lib/postgresql`, while the `pgvector/pgvector:pg16` image keeps `PGDATA` at `/var/lib/postgresql/data` and declares that path a `VOLUME`, so the data lives in an anonymous volume the `Makefile` backup (`Makefile:352-362`) never sees. Files: `docker-compose.yml` (the mount should be `/var/lib/postgresql/data`, or `PGDATA` moved under the named volume), `Makefile` (`backup`, `restore`).

**P19. `ollama-init` can only reach the bundled Ollama (row 59).**
`ollama-init` (profile `bundled-ollama`) pulls the configured models through `OLLAMA_BASE_URL`; on the `internal` network it cannot reach a host Ollama (`host.docker.internal` is not routable there), so with the default `.env` pointing at the host it exits without pulling. The pass ran it with `OLLAMA_BASE_URL=http://ollama:11434` against the bundled Ollama, where it pulled `llama3.2:1b` and `nomic-embed-text` (`59-ollama-init.log`, `59-bundled-ollama-list.txt`) and the Models page listed them (`59-models-page-bundled-ollama.png`). Files: `docker-compose.yml` (`ollama-init` service), `scripts/ollama-init.sh`.

**P20. `create_reminder` reports a past `due_at` as a format error, and the model chases the format (rows 45, 45b, 45c).**
Repro: call `create_reminder` with a well-formed RFC 3339 `due_at` a few minutes in the past. Expected: an error that names the cause ("due_at 19:23:00+12:00 is in the past; now is 19:29:01+12:00"). Observed: `Error: Provide nonblank content and a future RFC3339 due_at with an explicit UTC offset`, the same text as for a malformed value. In chat `2099faeb-…` (45b) the model then tried `+12:00`, `Z` and `+00:00` spellings of the same past instant, and in chat `1f14f15e-…` (45c) three past times in a row, before giving up or moving the time by an hour. On a 12 tokens/s model whose turn takes minutes, "a few minutes from now" is routinely in the past by the time the call is made. File: `runner/zone_server/src/db/reminders.rs` (the validation message). Related: the tool refuses `FREQ=MINUTELY` ("FREQ=MINUTE is not supported. Use HOURLY, DAILY, WEEKLY or MONTHLY", `runner/zone_server/src/services/schedule.rs:111`), so a condition watch cannot poll faster than hourly.

**P21. A "URL / Web Page" knowledge entry is stored as text and never fetched (row 54).**
Repro: Wiki, + Add Knowledge, choose URL / Web Page, enter `https://example.com/`, name it, Create Entry. Expected: an entry whose page is fetched, indexed and refreshable. Observed: a card badged `text` whose content is the literal URL, with no Refresh button (`54-text-entry-card.png` shows the same layout; the database row has `source_url` null, `content = 'https://example.com/'`, `last_fetched_at` null for both `Example page …` entries). The wizard posts `{type: "url", content: "<url>"}` and the server's `CreateKnowledgeRequest` has no `type` field, only `source_url`, so the URL lands in `content`. Files: `manager/frontend/src/features/knowledge/components/CreateKnowledgeWizard.tsx` (`handleComplete`), `manager/frontend/src/api/knowledge.ts`, `runner/zone_server/src/routes/context.rs` (`CreateKnowledgeRequest`).

**P22. The console's chat search never returns anything (row 33).**
Repro: type a word from any chat into "Search messages..." and press Enter. Expected: matching messages. Observed: no results, three runs out of three. The console calls `GET /api/chats/search?query=…&limit=20` and the server answers `400 Failed to deserialize query string: missing field workspace_id`; with `workspace_id` added the same query returns the message (`similarity 0.268` for `zebra-mu9jj0mr2x1`). Files: `manager/frontend/src/api/chats.ts` (`searchChatMessages` sends no `workspace_id`), `runner/zone_server/src/routes/chats.rs:875`.

**P23. With Ollama down, the Models page shows the ComfyUI inventory as if nothing were wrong (row 16).**
Repro: stop Ollama, open Models, Refresh. Expected: "Cannot connect to Ollama" with Retry (the page has that state). Observed: `GET /api/models` answers 200 with only the ComfyUI-side entries (`ace_step_v1_3.5b.safetensors`, FLUX, the adapters...), the chat models silently disappear and no error is shown (`16-ollama-stopped.png`); after Ollama returns, Refresh brings them back. Files: `runner/zone_server/src/routes/models/mod.rs` (the merged list swallows the Ollama error), `manager/frontend/src/features/models/pages/ModelsPage.tsx` (error state only on a failed request).

**P24. MCP tools reach chats but never a task run (row 32).**
With `ZONE_MCP_ENABLED=true` and `magents` on `PATH` the server connects the server and attaches its 22 tools to every chat turn (`INFO zone_core::mcp::client: Connected MCP server server=magents tools=22`, `Attached MCP tools to chat tools=22`). A task run gets none of them: `ChatTools::for_task` assembles its tools with `connect = false` (`runner/zone_server/src/agent/tools.rs`, `for_task` → `assemble(scope, ToolProfile::Task, Some(cwd), false)`), so a task told to hand work to another coding agent searched its catalog six times (`search_tools` answered `assess_pull_requests`, `get_build_status`, `get_task_run`) and never saw a `magents_*` tool (task `e04bda50-…`, run logs in evidence row 32). Files: `runner/zone_server/src/agent/tools.rs`.

### Observations that are not defects

- With Auto-approve on, the chat agent runs whatever `run_shell` it decides on: chasing a missing magents transcript (row 32b) it listed and read files under `~/.claude-personal/projects`, the local Claude session store. Nothing in the console distinguishes that from any other command.
- Chat tools write relative paths into the server process's working directory unless `ZONE_CHAT_AGENT_CWD` is set (row 40); on a rig started from a checkout that is the checkout itself. The pass added `ZONE_CHAT_AGENT_CWD` to `scripts/live-verify.sh`.
- `load_tools` answers "No deferred tool is named start_task" for a tool whose schema is already in front of the model (row 51); saying "start_task is already loaded, call it" would have ended both failed chats.
- The Models page's disk meter and the details modal read fine; `create_reminder`'s error for a past time, `update_task`'s missing priority field and the hourly floor on watches are in P20.


## 4. What this machine lacked

| Row | What was missing | What would clear it |
|---|---|---|
| 2, 5, 6 (mail path) | An SMTP relay. The server's mail transport is `SmtpTransport::relay` (implicit TLS with a verified certificate), so a local catcher cannot stand in; the pass took the documented no-mail path and read the token rows from the database. | `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM` for a real relay. |
| 10 | OpenAI and Anthropic API keys. The provider forms render and the LiteLLM section saves; no chat turn was answered by either provider. | An OpenAI key and an Anthropic key. |
| 21 | GitLab, Notion and Slack tokens. The GitLab wizard was exercised with a bad token (and the server has no GitLab adapter, see P9); Notion and Slack have no wizard entry. | Tokens for each, plus the missing adapters and wizard entries. |
| 38 (video) | Wan 2.2 TI2V 5B produces blurred colour fields on this ComfyUI 0.34.0 / PyTorch 2.9.1 / MPS build, in direct renders and in the earlier passes' outputs; Zone's own video path (submit, poll, collect, play, upscale) is not what fails. | A ComfyUI/PyTorch build on which the fp16 Wan weights sample correctly on MPS, or an NVIDIA host. |
| 18 (first attempt) | Memory. The first training run ended in `training loss became NaN` while the machine was swapping 20 GB with the 27B chat model resident (Ollama's default 262,144-token context put its runner at 36 GB). The rerun alone passed in 21 minutes. | A larger machine, or the context cap the pass applied (`OLLAMA_CONTEXT_LENGTH=32768`, `OLLAMA_NUM_PARALLEL=1`) from the start. |
| 61 (VPN) | VPN credentials for the `vpn` compose profile. SearXNG ran alone on 127.0.0.1:8089 and `SEARCH_SEARXNG_QUERY_URL` pointed the stack's manager at it, so the tool itself was judged. | `VPN_*` credentials in `.env` and `make up-vpn`. |
| 57 to 62 (stack build) | The compose stack does not build from `main` (P17); the manager image was built from a scratch copy of `manager/Dockerfile` with the missing `COPY runner/zone_notify` line, outside the repository. | The one-line Dockerfile fix. |
| 59 | The stack ran with the host Ollama, so `ollama-init` (bundled-ollama profile) never starts; the row was driven by starting the bundled Ollama and its init on demand. | `PROFILES=bundled-ollama` on a host where a containerised Ollama has a GPU. |
| 63 (`zone run`) | Nothing on the machine: the CLI cannot log in (P15), so `zone run`, `zone resume` and `zone sessions` had nothing to show. | The CLI fix. |
| 45 (condition watch) | Nothing missing on the machine, but the reminder tool's smallest cadence is hourly (`FREQ=MINUTELY` is refused, P20), so a watch can only be observed across a full hour; the pass set one hourly watch and waited for its second firing (row 45c in `evidence.jsonl`). | A finer cadence, or a test window of several hours. |
| 19 | Nothing missing, but an adapter is used only when the operator selects it: `COMFYUI_CHECKPOINT` names the adapter file, and there is no per-chat, per-workspace or trigger-word selection, so the first run (base checkpoint selected) drew a man in a red jacket for `zrkxyz`. The row passed after the server was restarted with the adapter selected. | A way to pick an adapter from the console or the chat. |

