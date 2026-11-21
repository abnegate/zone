# Spend reports and budgets

Spend reports show what an organization spends on AI through Zone's own
LiteLLM, by day, workspace, person, model and provider. Budgets set a monthly
limit for the organization, a workspace or one person: Zone notifies admins
when spend passes a warning threshold, and with a hard limit it stops sending
requests once the budget is used up.

Zone counts spend from LiteLLM's own spend logs. It sends each request under a
LiteLLM key it derives for whoever the request is for, reads LiteLLM's logs
every minute, and keeps the rows sent under its keys. Traffic that doesn't go
through Zone's LiteLLM is never given a number; the report lists it by reason
instead.

Organization owners and admins find reports and budgets under **Settings →
Organization → Spend**.

## Licensing

Spend reports and budgets are part of Zone Team, which Zone Enterprise
includes. Changing a budget needs a Team license that verifies and is either
active or in its 30-day grace period after expiry. You install the license
under **Settings → This server → License**.

| License state | Attribution, ingestion and hard limits | Reports and CSV | Budgets |
|---|---|---|---|
| Team or Enterprise, active or in grace | Running | Available | Editable by organization owners and admins |
| Lapsed (past grace) | Still running | Available | Read-only, with a lapsed notice |
| Stored key that no longer verifies | Still running | Available | Read-only, with a lapsed notice |
| Removed, or never installed (Community) | Stopped | An upgrade notice; the report is `null` and the CSV is refused | Changes refused |

A lapsed license never loosens a budget: reports stay readable and hard
limits stay enforced, but nobody can create, change or delete a budget until
the license is renewed. Every change is refused with `license_required` until
then, including one that would only raise a limit.

Removing the license is different: the server runs as Community, and
everything stops. Requests go to LiteLLM under the instance's `LITELLM_KEY`
again, nothing is ingested, and no hard limit is enforced. Zone keeps the
budgets and the spend it already counted, and picks up again as soon as a
license is installed.

One function decides all of this: `standing()` in
`runner/zone_server/src/services/spend/standing.rs`. No other spend code reads
the license.

- **Enforced** means spend is attributed, ingested, reported and held to hard
  limits. It holds while any license key is stored, including a lapsed one and
  one that no longer verifies, the same rule [agent policy](POLICY.md) uses.
- **Entitled** means budgets may be changed. It holds for a key that verifies
  and is active or in grace.

If Zone can't read the license, it treats spend as neither enforced nor
entitled until it can, and logs the error. Requests keep going, uncounted and
unblocked.

## Who can see and change

| What | Who |
|---|---|
| Reports and the CSV export | Organization owners and admins |
| Budgets, to see | Organization owners and admins |
| Budgets, to change | Organization owners and admins, with an entitled license |
| Budget notices | Active organization owners and admins, and for a workspace budget, that workspace's owners and admins too |

Other members of the organization, workspace admins included, get `403` "Only
organization owners and admins can see spend or change budgets". Someone who
isn't a member gets `404` "Organization not found", as if the organization
didn't exist.

## What's counted

Zone counts a request when all of these hold:

- the server enforces spend (see [Licensing](#licensing));
- the request is for a workspace;
- it goes to the instance's LiteLLM, the one `LITELLM_HOST` names, under the
  instance's non-empty `LITELLM_KEY`, not under a key saved in AI settings;
- its route doesn't prefer a coding agent.

| Request | Counted as |
|---|---|
| A member's chat turn, over the WebSocket or sent through the REST API | That member, in the chat's workspace |
| Everything else Zone sends through LiteLLM for a workspace, such as task runs, chat titles, pull request subjects and repairs, and auto-project reviews | **Automation**, in that workspace |

A pull request repair that falls back from a coding agent to the instance's
LiteLLM is counted as Automation, and held to hard limits, like any other
request there.

The organization is always the workspace's. For each request Zone keeps what
LiteLLM logged:

| Field | From LiteLLM |
|---|---|
| Spend | `spend`, in US dollars, as LiteLLM priced it |
| Prompt and completion tokens | `prompt_tokens` and `completion_tokens` |
| Model | `model_group`, the name Zone asked for, or `model` when there's none |
| Provider | `custom_llm_provider`, or `unknown` |
| Time | `startTime`, when the request started |

Zone stores money as whole microdollars, millionths of a US dollar. It rounds
LiteLLM's figure half away from zero and counts a negative one as $0. The
console shows dollars and cents; the CSV shows six decimal places.

Spend for a workspace that's deleted keeps counting toward the organization's
totals and shows as **Deleted workspace**.

## What isn't counted

Traffic that bypasses Zone's LiteLLM, or can't be tied to a workspace, never
gets a number. The report's `uncounted` lists the reasons that apply in the
organization right now, worked out from the organization's and each
workspace's AI settings, and the console shows a line for each:

| Code | What | Why |
|---|---|---|
| `coding_agent` | Chats and tasks on Claude Code or Codex | They run on a subscription sign-in, not through LiteLLM. This includes an instance whose `ZONE_LLM_BACKEND` is `claude` or `codex`. |
| `direct_provider` | The OpenAI or Anthropic provider in AI settings | Requests go straight to the provider with the saved key. |
| `external_endpoint` | A Self-Hosted endpoint on another host | Requests go to a server other than Zone's LiteLLM. |
| `saved_key` | A key saved in AI settings for the instance's LiteLLM host | The requests carry the saved key, so Zone can't tell whose they are. |

These are never counted either, and the console always lists them in its
footnote:

- chats that pick their own provider;
- requests outside a workspace;
- embeddings, which go to Ollama directly by default, or to LiteLLM under
  `LITELLM_KEY`.

A request is also sent uncounted while its key can't be registered with
LiteLLM (see [Registration](#registration)), and everything is while the
server runs as Community.

For coding-agent subscription usage, the Spend page links to AI settings
instead.

### Local models at $0

LiteLLM has no price for local models, so requests to Ollama are counted with
their tokens at $0. To give them a price, add `model_info` to their routes in
`litellm/config.yaml.template`, from which LiteLLM's `config.yaml` is
generated. Prices are US dollars per token:

```yaml
model_list:
  - model_name: reason
    litellm_params:
      model: ollama_chat/{{OLLAMA_MODEL_REASON}}
      api_base: {{OLLAMA_BASE_URL}}
      timeout: 180
    model_info:
      input_cost_per_token: 0.0000001
      output_cost_per_token: 0.0000004
```

This prices `reason` at $0.10 per million prompt tokens and $0.40 per million
completion tokens. Each route is priced on its own, so price every route your
chats use, including the stock `"*"` route that serves model names with no
route of their own. LiteLLM prices a request when it logs it, so a price
applies from when LiteLLM loads it; earlier requests stay at $0.

## How attribution works

### Derived keys

LiteLLM's spend logs name the key each request was sent under, as the SHA-256
of the key. So Zone sends each request under a LiteLLM virtual key of its own
for whoever the request is for: one per member in each workspace, and one for
each workspace's automation.

The key is `sk-zone-` followed by the hexadecimal HMAC-SHA256 of
`zone-spend-key:v1:{workspace id}:{user id or automation}`, keyed with key
material derived from `ENCRYPTION_KEY`. Zone never stores it: it derives it
again for each request. The `spend_keys` table keeps only the key's SHA-256,
its token, beside the organization, workspace and member it stands for.

The key comes from `ENCRYPTION_KEY`, not `LITELLM_KEY`, so LiteLLM, which
holds `LITELLM_KEY`, never has what it would need to work out a key.

### Registration

The first time a key is needed, Zone registers it with `POST /key/generate`,
under the master key, sending the `key` and `metadata` naming its
`zone_organization`, `zone_workspace` and `zone_user`. It sends:

- no `key_alias`, since LiteLLM refuses an alias it already has;
- no `user_id`, since an existing LiteLLM user could narrow the models the key
  may call;
- no `models`, so the key may call every model, including the `"*"` route.

LiteLLM updates a key it already has, so registering one again is harmless.
Zone waits up to 2 seconds, and LiteLLM's answer must carry the key's token.
If registration fails, the request goes under `LITELLM_KEY` as before: it
isn't counted, but it is still held to hard limits. Zone doesn't try that key
again for 30 seconds.

Each Zone process remembers in memory which keys it has registered with which
LiteLLM, and sends a registered key without asking LiteLLM first. A restarted
server registers each key again the first time it's used, which also restores
keys LiteLLM lost to a database restore or a manual delete. Every 15 minutes
the spend sweep checks each key the process has registered with
`GET /key/info`, and registers again any LiteLLM no longer has.

### Changing `ENCRYPTION_KEY`

A new `ENCRYPTION_KEY` derives new keys, which Zone registers as they're
needed. Every 15 minutes the sweep deletes from LiteLLM, with
`POST /key/delete` in batches of 100, every key the current `ENCRYPTION_KEY`
no longer derives, and marks it revoked. Zone keeps its `spend_keys` row, so
spend LiteLLM logged under it still counts.

When a workspace is deleted, its `spend_keys` rows go with it, and its keys
are left in LiteLLM. Its spend already counted stays, as **Deleted
workspace**.

### When LiteLLM's database is down

LiteLLM checks a virtual key against its own database. While that database is
down, it refuses virtual keys unless `general_settings` sets
`allow_requests_on_db_unavailable: true`; the master key keeps working. So
with spend enforced, an outage of LiteLLM's database stops attributed
requests, where before they kept going. In the compose stack, LiteLLM's
database is on the same Postgres server as Zone's, so an outage usually stops
Zone too.

Zone doesn't change this setting. Turning it on lets LiteLLM accept keys it
can't check, which is a security trade-off, so it's the operator's choice.

### LiteLLM settings that break attribution

- **`general_settings.disable_spend_logs: true`.** LiteLLM writes no spend
  logs, so Zone counts nothing and no hard limit ever trips.
- **`litellm_settings.default_key_generate_params`** and
  **`upperbound_key_generate_params`.** LiteLLM applies these to every key it
  generates, Zone's included. `models` there limits which models attributed
  requests may call, and settings such as `duration` or `max_budget` make
  Zone's keys expire or stop. Leave them unset.

Zone needs nothing from LiteLLM Enterprise: `/spend/logs/v2` and key
generation with `metadata` are in LiteLLM's open-source proxy. The stock
`store_prompts_in_spend_logs: false` keeps prompts out of the spend logs, and
Zone doesn't need them.

## Ingestion and lag

Every 60 seconds, the `spend-ingest` sweep copies new spend from LiteLLM. It
runs when:

- `ZONE_LLM_BACKEND` is `litellm`, the default;
- the server enforces spend;
- `LITELLM_HOST` and `LITELLM_KEY` are both set.

One instance ingests at a time, under a database advisory lock. Each run
reads `GET /spend/logs/v2` 1,000 rows a page, oldest first, until a page comes
back short. LiteLLM stops counting its `total` at 10,000, so Zone never
relies on it. Zone stores only the rows sent under one of its keys, by
`request_id`, and drops the rest.

### The overlap

A run reads from 35 minutes before where the last run ended to the moment it
started. When it succeeds, the next run picks up from that moment; when it
fails, the next run reads the same window again. The first run starts 35
minutes before Zone derived its first key. With no keys, a run does nothing.

The overlap exists because LiteLLM logs a request once it finishes, under the
time it started, and writes logs in batches about every 10 seconds. Zone gives
up on a completion after 30 minutes, so a request can appear in the log up to
30 minutes after the time it's filed under; the other 5 minutes leave room for
LiteLLM to write it. A request LiteLLM logs more than 35 minutes after it
started is missed.

Re-reading never counts a request twice: a request Zone already has takes
LiteLLM's latest spend, tokens and outcome.

**Every run re-reads the whole 35-minute window. That's expected.** Zone reads
every row in the window, under any key, and keeps its own, so the cost grows
with all of LiteLLM's traffic. At 1,000 requests a minute through LiteLLM,
each run reads about 35,000 rows, about 35 pages. Many pages a run doesn't
mean ingestion is behind.

### Lag and overshoot

A request shows up in reports after it finishes, once LiteLLM writes its log
and the next run reads it: usually within about a minute. The report's
`ingested_through`, shown as **Counted through**, is where the last
successful run ended.

Hard limits work from the same figures, so a budget can go over its limit.
After each run the sweep marks every budget whose spend has reached its
limit, and Zone checks that mark when a request starts. Requests already
running when the budget runs out finish. New ones keep starting until a run
has counted the spend that used it up: up to about 70 seconds after those
requests finish, the 60-second cadence plus LiteLLM's 10-second batching.

## Budgets

A budget is a monthly limit on one target. Each target has at most one
budget.

| Scope | Counts | A hard limit stops |
|---|---|---|
| `organization` | All the organization's counted spend: every workspace, every member and automation, and deleted workspaces | Every counted request in the organization |
| `workspace` | The workspace's counted spend: its members' and its automation's | Every counted request in that workspace |
| `user` | That member's chat turns, in every workspace of the organization | Only that member's chat turns |

| Setting | Values |
|---|---|
| Limit | More than $0. The API takes `limit_microdollars`; the console takes dollars. |
| Warning threshold | A whole percentage from 1 to 100. The default is 80. |
| Hard | Whether to stop requests once the budget is used up. Off by default. |

A budget is in one of three states:

| State | Console | When |
|---|---|---|
| `under` | On track | Spend is below the threshold |
| `warning` | Past its warning | Spend is at least the threshold's share of the limit |
| `exhausted` | Used up | Spend is at least the limit |

### Months

Budgets run on UTC calendar months. Each month counts the requests that
started from 00:00 UTC on the 1st, and every budget starts again on the 1st.
A report's days are UTC days too.

### Hard limits

A hard limit stops only requests Zone counts: requests through the instance's
LiteLLM under Zone's keys. Coding agents, direct providers and other endpoints
keep running. A request whose key couldn't be registered is still stopped,
even though it isn't counted.

Zone checks when a chat turn or a run starts, not partway through. A member's
own budget stops only that member's chat turns; organization and workspace
budgets stop automation as well. When more than one used-up hard budget
covers a request, Zone names the broadest: the organization's, then the
workspace's, then the member's. A request the agent policy refuses is
reported as a policy refusal, not a budget one.

If Zone can't read the budgets when a request starts, it lets the request go
and logs the error. Unlike agent policy, budgets fail open.

A change takes effect at once. Creating or changing a budget works out its
state again, so a hard budget created below the month's spend stops requests
right away, and raising a limit above it lets them through right away.

### Notices

Each budget gets one warning notice and one exhaustion notice a month. When
spend passes both between two runs, only the exhaustion notice goes. A soft
budget gets the same notices as a hard one.

A notice goes:

- by email, when SMTP is configured, to the organization's active owners and
  admins, and for a workspace budget to that workspace's owners and admins
  too, one at a time, each waiting up to 60 seconds;
- to the server's notification channels:
  [`ZONE_NOTIFY_SLACK_WEBHOOK`](CONFIGURATION.md#zone_notify_slack_webhook),
  [`ZONE_NOTIFY_DISCORD_WEBHOOK`](CONFIGURATION.md#zone_notify_discord_webhook)
  and [`ZONE_NOTIFY_EMAIL_TO`](CONFIGURATION.md#zone_notify_email_to).

A notice counts as sent when at least one channel delivered it, or when no
channel is configured. If every configured channel fails, the next run tries
again.

A notice is re-armed once its condition stops holding. Raising a limit so
spend falls below the threshold re-arms the warning, and a later crossing that
month sends it again. Lowering a limit or a threshold can send a notice at the
next run.

## What people see

| Where | What happens |
|---|---|
| Chat turn, over the WebSocket | The turn doesn't start, and ends with `{"type": "error", "message": "…"}`. |
| Sending a message through the REST API | `409` with `{"error": "…"}`. |
| Task runs and other automation | The run fails with the message. |

The message says whose budget is used up:

| Budget | Message |
|---|---|
| Organization | Your organization's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC). |
| Workspace | This workspace's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC). |
| Member | Your monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC). |

The refusal's code is `budget_exhausted`. Zone logs each refused request but
doesn't audit it.

The Spend page shows month to date by default, with a range, workspace and
person filters, a CSV download, totals, a daily chart, breakdowns by
workspace, person, model and provider, what isn't counted, and the monthly
budgets. Spend no member sent from a chat shows as **Automation**.

## Audit log

Zone writes these entries to the organization's audit log.

| Action | When | Resource | Recorded values |
|---|---|---|---|
| `budget.created` | A budget is created | `budget` | The new budget |
| `budget.updated` | A budget is changed | `budget` | The budget before and after |
| `budget.deleted` | A budget is deleted | `budget` | The budget before |
| `spend_export.downloaded` | Someone downloads the CSV | `spend_export`, with the organization's ID | `from`, `to`, `workspace_id` and `user_id` |
| `budget.threshold_reached` | A warning notice goes out; recorded by Zone, with no actor | `budget` | `month`, `spent_microdollars`, `limit_microdollars` and `hard` |
| `budget.exhausted` | An exhaustion notice goes out; recorded by Zone, with no actor | `budget` | `month`, `spent_microdollars`, `limit_microdollars` and `hard` |

Saving a budget without changing it writes no entry. Refused requests aren't
audited.

## Codes

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | This needs a Zone Team license that is active or in its grace period. See [Licensing](#licensing). |
| `spend_body_invalid` | 400 | The request body can't be read: it isn't valid JSON, has a key that isn't a budget field, or has a value of the wrong type. |
| `spend_range_invalid` | 400 | `from` or `to` isn't a `YYYY-MM-DD` date, or `from` is after `to`. |
| `budget_limit_invalid` | 400 | The limit is $0 or less. |
| `budget_threshold_invalid` | 400 | The threshold isn't a whole percentage from 1 to 100. |
| `budget_scope_invalid` | 400 | The scope doesn't carry exactly the ID it needs, or names a workspace or person outside the organization. |
| `budget_exists` | 409 | The target already has a budget. |
| `budget_exhausted` | | A request was stopped by a used-up hard budget. See [What people see](#what-people-see). |

These responses carry no code:

| Status | Message |
|---|---|
| 403 | Only organization owners and admins can see spend or change budgets |
| 404 | Organization not found |
| 404 | Budget not found |

## API

| Method and path | Purpose |
|---|---|
| `GET /api/organizations/{org_id}/spend-reports` | A report for a date range, and licensing state. |
| `GET /api/organizations/{org_id}/spend-exports` | The same range as CSV. |
| `GET /api/organizations/{org_id}/budgets` | The organization's budgets, with this month's spend. |
| `POST /api/organizations/{org_id}/budgets` | Create a budget. |
| `PATCH /api/organizations/{org_id}/budgets/{budget_id}` | Change a budget. |
| `DELETE /api/organizations/{org_id}/budgets/{budget_id}` | Delete a budget. |

A read is checked in this order: membership (404), the caller's role (403),
then the range (400 `spend_range_invalid`). A change is checked in this
order: membership (404), the caller's role (403), the license (403
`license_required`), the body (400), the target (400
`budget_scope_invalid`), then whether the target already has a budget (409
`budget_exists`).

Every `GET` that answers JSON carries the licensing state:

| Field | Meaning |
|---|---|
| `entitled` | The license allows changing budgets: active or in grace. |
| `enforced` | The license enforces spend: any stored license key, including a lapsed one or one that no longer verifies. |
| `edition` | The license's edition, such as `community` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `editable` | The license allows changing budgets and you may change them. |

### Reports

```http
GET /api/organizations/{org_id}/spend-reports?from=2026-11-01&to=2026-11-21&workspace_id={ws_id}&user_id={user_id}
```

| Parameter | Value |
|---|---|
| `from` | A `YYYY-MM-DD` date in UTC. Defaults to the first of the current month. |
| `to` | A `YYYY-MM-DD` date in UTC, included. Defaults to today. Not before `from`. |
| `workspace_id` | Optional. Only that workspace's spend. |
| `user_id` | Optional. Only that person's spend. |

Filters narrow the organization's spend, so a workspace or person from
another organization gives an empty report.

```json
{
  "entitled": true,
  "enforced": true,
  "edition": "enterprise",
  "status": "active",
  "editable": true,
  "report": {
    "from": "2026-11-01",
    "to": "2026-11-21",
    "ingested_through": "2026-11-21T10:04:00Z",
    "total": {"spend_microdollars": 12340000, "requests": 820, "prompt_tokens": 1450000, "completion_tokens": 310000},
    "daily": [
      {"date": "2026-11-01", "spend_microdollars": 0, "requests": 0, "prompt_tokens": 0, "completion_tokens": 0}
    ],
    "workspaces": [
      {"id": "…", "name": "Payments", "spend_microdollars": 9100000, "requests": 610, "prompt_tokens": 1100000, "completion_tokens": 240000}
    ],
    "users": [
      {"id": null, "name": null, "email": null, "spend_microdollars": 4200000, "requests": 300, "prompt_tokens": 520000, "completion_tokens": 90000}
    ],
    "models": [
      {"name": "gpt-4o", "spend_microdollars": 12340000, "requests": 820, "prompt_tokens": 1450000, "completion_tokens": 310000}
    ],
    "providers": [
      {"name": "openai", "spend_microdollars": 12340000, "requests": 820, "prompt_tokens": 1450000, "completion_tokens": 310000}
    ],
    "uncounted": ["coding_agent", "saved_key"]
  }
}
```

| Field | Meaning |
|---|---|
| `report` | `null` while the server doesn't enforce spend. |
| `ingested_through` | Where the last successful ingest run ended, or `null` before the first. |
| `total` | Spend, requests and tokens over the range. |
| `daily` | One entry per UTC day of the range, with days that had nothing as zeros. |
| `workspaces` | Per workspace. A `null` ID is a deleted workspace. |
| `users` | Per person. A `null` ID is Automation. |
| `models` | Per model: LiteLLM's `model_group`, or `model` without one. |
| `providers` | Per provider: LiteLLM's `custom_llm_provider`, or `unknown`. |
| `uncounted` | The [reasons](#what-isnt-counted) traffic in the organization goes uncounted. |

Breakdowns are sorted by spend, highest first, then by name.

### CSV export

`GET /api/organizations/{org_id}/spend-exports` takes the same parameters,
and answers `text/csv; charset=utf-8` with `Content-Disposition: attachment;
filename="zone-spend-{from}-{to}.csv"`. It's refused with `license_required`
while the server doesn't enforce spend, and every download is recorded as
`spend_export.downloaded`.

The header row is:

```
date,workspace_id,workspace,user_id,user_email,model,provider,requests,prompt_tokens,completion_tokens,spend_usd
```

There's one row per day, workspace, person, model and provider, with CRLF
line ends. `spend_usd` has six decimal places. Empty `user_id` and
`user_email` are Automation, and an empty `workspace_id` is a deleted
workspace. A text field starting with `=`, `+`, `-`, `@`, a tab or a carriage
return is quoted and prefixed with `'`, as in the
[audit log CSV](AUDIT.md#downloading-the-audit-log).

### Budgets

`GET /api/organizations/{org_id}/budgets` answers the licensing state, the
current UTC month as `"month": "2026-11"`, and `budgets`. Zone returns stored
budgets whatever the license.

```json
{
  "id": "…",
  "scope": "workspace",
  "workspace_id": "…",
  "workspace_name": "Payments",
  "user_id": null,
  "user_name": null,
  "user_email": null,
  "limit_microdollars": 50000000,
  "threshold_percent": 80,
  "hard": true,
  "spent_microdollars": 41200000,
  "state": "warning",
  "updated_at": "2026-11-21T10:04:00Z"
}
```

| Field | Meaning |
|---|---|
| `scope` | `organization`, `workspace` or `user`. |
| `workspace_id`, `workspace_name` | The workspace a `workspace` budget covers, otherwise `null`. |
| `user_id`, `user_name`, `user_email` | The person a `user` budget covers, otherwise `null`. |
| `limit_microdollars` | The monthly limit. |
| `threshold_percent` | The warning threshold. |
| `hard` | Whether the budget stops requests once used up. |
| `spent_microdollars` | This month's spend so far. |
| `state` | `under`, `warning` or `exhausted`. See [Budgets](#budgets). |

`POST /api/organizations/{org_id}/budgets` creates a budget and answers `201`
with it:

```http
POST /api/organizations/{org_id}/budgets
Content-Type: application/json

{"scope": "workspace", "workspace_id": "…", "limit_microdollars": 50000000, "threshold_percent": 80, "hard": true}
```

| Field | Value |
|---|---|
| `scope` | Required: `organization`, `workspace` or `user`. |
| `workspace_id` | Required for `workspace`, and a workspace in the organization; not allowed otherwise. |
| `user_id` | Required for `user`, and a member of the organization; not allowed otherwise. |
| `limit_microdollars` | Required, more than 0. |
| `threshold_percent` | Optional, 1 to 100. Defaults to 80. |
| `hard` | Optional. Defaults to `false`. |

`PATCH /api/organizations/{org_id}/budgets/{budget_id}` carries only the
fields you're changing, from `limit_microdollars`, `threshold_percent` and
`hard`, and answers `200` with the budget. A budget's target can't change:
delete it and create another.

`DELETE /api/organizations/{org_id}/budgets/{budget_id}` answers `204`.
