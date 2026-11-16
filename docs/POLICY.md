# Agent and model policy

An agent policy limits what the agents in an organization or workspace may
use: which model providers and coding agents run its chats and tasks, which
models they ask for, which tool calls Auto-approve may run without a person,
which MCP servers they get, and which domains they may reach. Zone checks the
policy on the server every time a chat turn, a task run or a background
completion starts, picks a model or runs a tool, and when someone signs an
agent in or saves AI settings. The console only shows what the server already
enforces.

An organization admin sets the organization's policy under **Settings →
Organization → Agent policy**. A workspace can narrow it under **Settings →
Workspace → Agent policy**, but never widen it.

## Licensing

Agent policy is part of Zone Team and Zone Enterprise. Editing a policy needs
a Team or Enterprise license that's either active or in its 30-day grace
period after expiry. You install the license under **Settings → This server →
License**.

| License state | Enforcement | Settings |
|---|---|---|
| Team or Enterprise, active or in grace | Enforced | Editable by the people listed [below](#who-can-see-and-change-a-policy) |
| Lapsed (past grace) | Still enforced | Read-only, with a renewal notice |
| Removed, or never installed (Community) | Not enforced | Read-only, with an upgrade notice |

A lapsed license never loosens a policy: everything it restricts stays
restricted, and nobody can change it until the license is renewed. Every
change is refused with `license_required` until then, including a change that
would only loosen the policy.

Removing the license is different: the server runs as Community, and
Community enforces no policy. Zone keeps the saved policies, and enforces them
again as soon as a Team or Enterprise license is installed.

A stored license key that no longer verifies, for example after an upgrade
changes the keys Zone trusts, isn't treated as removed. The saved policies stay
enforced, and every change is refused with `license_required` until an admin
installs a key that verifies or removes the license.

If Zone can't read the license or a policy when it needs it, it refuses
instead of running unrestricted. The refusal is `policy_unreadable`; a task
run retries it, and in a chat you can try again shortly.

## Who can see and change a policy

| Policy | Who can see it | Who can change it |
|---|---|---|
| Organization | Every active member of the organization | Organization owners and admins |
| Workspace | Members of the workspace, and organization owners and admins | Workspace owners and admins, and organization owners and admins |

Someone who can see a policy but not change it sees it read-only. Someone who
isn't a member gets 404, as if the organization or workspace didn't exist.

## How the organization and workspaces combine

A policy has six fields. Each one is either unset, which means "this layer
doesn't restrict it", or set, which restricts it. An empty list is set: it
allows nothing.

| Field | What it restricts | Values |
|---|---|---|
| `providers` | Where model completions run | `local`, `anthropic`, `openai`, `bedrock`, `vertex`, `azure`, `other` |
| `agents` | Which coding agents may run | `claude`, `codex` |
| `models` | Which model names may be asked for | Model names and patterns |
| `ceiling` | Which tool calls Auto-approve may run without a person | `write` or `host` |
| `mcp_servers` | Which MCP servers agents get | Server names |
| `domains` | Which hosts agents may reach | Domains and IP addresses |

A workspace's chats and tasks run under both layers at once: a value has to be
allowed by every layer that sets the field. A workspace that sets nothing runs
under the organization's policy alone.

A workspace may only narrow the organization's policy. When you save a
workspace field, Zone checks it against the organization's:

- `providers`, `agents` and `mcp_servers` must be a subset of the
  organization's list;
- every model pattern and every domain must be covered by one of the
  organization's;
- the ceiling can't be higher than the organization's;
- anything goes for a field the organization leaves unset.

A save that would widen a field is refused with
`policy_wider_than_organization`. Zone checks only the fields you save. If the
organization narrows a field later, Zone doesn't rewrite the workspace's
setting, and both layers keep being enforced, so the narrower one wins.

In the console, the workspace section shows the organization's value, the
workspace's own value and the effective result side by side. Options the
organization doesn't allow are disabled and marked "Not allowed by your
organization". **Use organization policy** clears the workspace's own value
for a field.

### Common policies

| Goal | Policy |
|---|---|
| No personal Claude or ChatGPT sign-ins | `agents: []` |
| Claude only through your own AWS or Google Cloud account | `providers: ["bedrock", "vertex"]`, `agents: []` |
| Only local models in one sensitive workspace | On that workspace: `providers: ["local"]`, `agents: []` |
| Never auto-approve shell commands or file writes on the host | `ceiling: "write"` |
| No network access for agents beyond your own hosts | `domains: ["example.com"]` |

## Providers

`providers` restricts where a completion actually runs. Zone works it out
from the provider chosen in AI settings and, for a LiteLLM endpoint, from the
route LiteLLM uses for the model.

| Value | Label in the console | Matches |
|---|---|---|
| `local` | Local models | LiteLLM routes with the prefix `ollama`, `ollama_chat`, `hosted_vllm`, `lm_studio` or `llamafile` |
| `anthropic` | Anthropic API | The **Anthropic** provider in AI settings, and LiteLLM routes with the prefix `anthropic` |
| `openai` | OpenAI API | The **OpenAI** provider in AI settings, and LiteLLM routes with the prefix `openai` or `text-completion-openai` |
| `bedrock` | AWS Bedrock | LiteLLM routes with the prefix `bedrock` |
| `vertex` | Google Vertex AI | LiteLLM routes with the prefix `vertex_ai` or `vertex_ai_beta` |
| `azure` | Azure OpenAI | LiteLLM routes with the prefix `azure` or `azure_ai` |
| `other` | Other OpenAI-compatible endpoints | Every other route, and every route Zone can't identify |

The prefix is the part of a LiteLLM route's `model` before the first `/`. A
route with no prefix, or a prefix that isn't in the table, counts as `other`.
So does Google AI Studio (`gemini/`): only `vertex_ai` routes count as Vertex
AI.

Each provider in AI settings is checked this way:

| Provider in AI settings | Checked against |
|---|---|
| Self-Hosted | The model's LiteLLM route |
| AWS Bedrock | The model's LiteLLM route, since it runs on the instance's LiteLLM |
| OpenAI | `openai` |
| Anthropic | `anthropic` |
| Claude Code | The `claude` agent (see [Coding agents and sign-ins](#coding-agents-and-sign-ins)) |
| Codex | The `codex` agent |

Zone classifies the OpenAI and Anthropic providers by the provider chosen in
AI settings, not by their base URL. Allowing `openai` allows whatever base URL
an admin saves for the OpenAI provider; use
[`ZONE_ENDPOINT_HOSTS`](CONFIGURATION.md#zone_endpoint_hosts) to limit the
hosts those URLs may name.

### How Zone tells where a LiteLLM model runs

When `providers` is set at either layer and a completion goes to a LiteLLM
endpoint Zone trusts, Zone asks that endpoint for its routes with
`GET {endpoint}/v2/model/info`, using the key Zone uses for the endpoint. It
then finds the route for the model name it's about to ask for:

1. a `model_name` that equals the model name exactly;
2. otherwise the longest wildcard `model_name` whose prefix matches, with `*`
   replaced by the model name.

The route's `model` prefix gives the provider. With the stock
`litellm/config.yaml.template`, `fast`, `reason` and `vision` route to
`ollama_chat/…`, and the `"*"` route sends every other name to
`ollama_chat/*`, so all of them count as `local`.

Zone caches each endpoint's routes for 60 seconds, so a change to LiteLLM's
`config.yaml` can take up to a minute to reach policy decisions. A lookup that
fails is remembered for 5 seconds, and lookups that arrive while one is in
flight wait for it rather than ask again.

**Zone believes only the routes of an endpoint the organization controls.**
The routes are the endpoint's own answer, so whoever chooses the endpoint
chooses what it claims. Zone trusts:

- the instance's own `LITELLM_HOST`, and a saved URL on the same host;
- an endpoint saved in the organization's AI settings.

An endpoint saved in a workspace's AI settings counts as `other` whatever its
routes say. Without this, a workspace admin could save a proxy that reports
every route as `ollama_chat/…` while forwarding to a hosted API, and satisfy an
organization's `providers: ["local"]`; a workspace may only narrow the
organization's policy, never get round it. To run a workspace on its own
LiteLLM endpoint under a `providers` restriction, save the endpoint at the
organization, or allow `other`.

**An upstream Zone can't identify counts as `other`.** That covers a model
with no route, an endpoint that isn't LiteLLM, an endpoint that doesn't answer
`/v2/model/info` within 2 seconds, a key that may not read it, and an endpoint
a workspace saved. `other` is denied unless the policy allows it, so a
restricted policy fails closed.

When `providers` is unset, Zone doesn't look up routes at all.

### LiteLLM routes for Bedrock, Vertex AI and Azure

To run models through your own cloud account, add routes to LiteLLM's
`config.yaml` (generated from `litellm/config.yaml.template`) and choose them
in AI settings as Self-Hosted models. Credentials stay in LiteLLM's
environment, never in Zone. The model IDs below are examples: use the IDs,
inference profiles and deployments your account has access to.

**AWS Bedrock** counts as `bedrock`:

```yaml
model_list:
  - model_name: claude-sonnet
    litellm_params:
      model: bedrock/anthropic.claude-sonnet-5-5
      aws_region_name: us-east-1
      aws_access_key_id: os.environ/AWS_ACCESS_KEY_ID
      aws_secret_access_key: os.environ/AWS_SECRET_ACCESS_KEY

  - model_name: claude-opus
    litellm_params:
      model: bedrock/anthropic.claude-opus-5-5
      aws_region_name: us-east-1
```

A cross-region inference profile, such as one beginning `us.anthropic.`, goes
after the `bedrock/` prefix in the same way. LiteLLM's other Bedrock route
forms, such as `bedrock/converse/…`, also count as `bedrock`.

**Google Vertex AI** counts as `vertex`:

```yaml
model_list:
  - model_name: claude-sonnet-vertex
    litellm_params:
      model: vertex_ai/claude-sonnet-5-5
      vertex_project: my-gcp-project
      vertex_location: global
      vertex_credentials: os.environ/VERTEX_CREDENTIALS_PATH
```

**Azure OpenAI** counts as `azure`. After `azure/` comes your Azure
deployment name:

```yaml
model_list:
  - model_name: gpt-azure
    litellm_params:
      model: azure/my-gpt-deployment
      api_base: https://my-resource.openai.azure.com
      api_version: os.environ/AZURE_API_VERSION
      api_key: os.environ/AZURE_API_KEY
```

A wildcard route works too. With this one, any model name beginning
`bedrock-` that has no route of its own runs on Bedrock:

```yaml
model_list:
  - model_name: "bedrock-*"
    litellm_params:
      model: bedrock/*
      aws_region_name: us-east-1
```

A name that matches no more specific route falls through to the stock `"*"`
route and counts as `local`.

## Coding agents and sign-ins

`agents` restricts the coding agent CLIs: `claude` (Claude Code) and `codex`
(Codex). These agents run only on consumer sign-ins: the organization's own
Claude or ChatGPT subscriptions, or the host's login. They're a separate list
from `providers` because they aren't a route to a model API. Allowing
`anthropic` or `bedrock` doesn't allow Claude Code, and `agents: []` doesn't
stop Claude models running through Bedrock or Vertex AI.

A denied agent is refused everywhere it could start:

- **Sign-ins.** Starting, finishing or redeeming a sign-in for a denied agent
  is refused with `policy_agent_denied`, and the agent's status shows why.
  Sign-ins belong to the organization, so only the organization's policy
  decides this.
- **AI settings.** Saving Claude Code or Codex as the provider is refused with
  `policy_agent_denied`, and saving a denied OpenAI or Anthropic provider is
  refused with `policy_provider_denied`. A workspace's save is checked
  against both layers; an organization's against its own policy.
- **Chats and tasks.** Zone never routes a turn, a task run or a mid-turn
  handover to a denied agent, and never falls back to the host's login for
  one. A chat or task whose only route is a denied agent is refused.

## Models

`models` restricts the model names Zone asks for. An entry matches a name
exactly, or, when it ends in a single `*`, every name that begins with the
rest. An entry is 1 to 128 characters, has no whitespace, and may only have
`*` as its last character; anything else is refused with
`policy_model_invalid`.

Entries match the name Zone asks for, not where it runs:

- on a LiteLLM endpoint, the `model_name` (`claude-sonnet` above, not
  `bedrock/anthropic.claude-sonnet-5-5`);
- on Claude Code, the agent's model names, such as `sonnet`, `opus` or
  `claude-opus-4-8`;
- on Codex, names such as `gpt-5.5`.

Combine `models` with `providers` when both matter: `models` alone can't tell
two routes with the same name apart.

**Automatic** asks a coding agent for no particular model, so the agent runs
its own default. Zone sends it only while every layer that sets `models`
lists the literal entry `auto`. A wildcard such as `*` or `a*` never allows
it; otherwise Zone picks the first of the agent's models the policy allows,
and refuses the turn when there's none.

Choosing a denied model is refused in AI settings, when changing a chat's
model, for a task's or an auto-project planner's model, and when a turn or
task run starts. Model lists in the console show denied models disabled, with
the reason.

Where Zone picks the model itself, it picks only among allowed ones. Chat
titles, pull request subjects, auto-project summaries and image-intent checks
try their usual candidates in order and use the first one the policy allows.
If none is allowed, Zone skips that completion and records the denial in the
audit log.
Auto-project reviews and pull request repairs fail with the denial instead.

### What a policy can't govern

- **An agent's own choice.** When the policy lists `auto` and Zone asks
  Claude Code or Codex for no particular model, the agent uses its own
  default, which Zone doesn't see.
- **Subagents.** An agent can start subagents on models of its own choosing,
  inside its own process.

To keep an agent off models you haven't approved, deny the agent.

## Tool approval ceiling

Every tool has a tier:

| Tier | What a call can do |
|---|---|
| Read | Observes and changes nothing. |
| Write | Changes state through Zone's audited workspace APIs, where it can be seen and put back. |
| Host | Writes files or runs commands where the agent runs. Nothing records what it did beyond the tool result. |
| Outward | Reaches a person or a system outside the workspace, such as a message, a pull request, the web or an MCP server. What it sends can't be recalled. |

Without a ceiling, a chat with Auto-approve off asks a person before every
Host or Outward call, and a chat with Auto-approve on asks for nothing.

The `ceiling` is the highest tier Auto-approve may run without a person. It
can only be `write` or `host`; `read` and `outward` are refused with
`policy_ceiling_invalid`. Unset means Auto-approve has no limit.

**A tool call waits for a person when its tier is Host or Outward, and either
Auto-approve is off or the call's tier is above the ceiling.** Read and Write
calls never wait.

| Ceiling | Console option | With Auto-approve on, asks before |
|---|---|---|
| Unset | No limit | Nothing |
| `host` | Ask before outward tools such as messages, pull requests, web and MCP | Outward calls |
| `write` | Ask before host tools such as shell and file writes | Host and Outward calls |

The effective ceiling is the lower of the organization's and the workspace's.
A ceiling also holds while calls are already waiting: turning Auto-approve on
releases only the waiting calls at or below the ceiling, and an approval that
times out under Auto-approve runs only a call at or below it.

### Agents' own tools

The ceiling also governs the tools coding agents ship with:

- **Claude Code** on a turn with Zone's tools sends its own tool calls through
  Zone's approval, so the ceiling applies to them. Its `WebFetch` and
  `WebSearch` count as Outward, and every other built-in tool as Host.
- **An agent that doesn't ask Zone before using its own tools,** such as Codex,
  or Claude Code on a turn without Zone's tools, has its own tools withheld
  under any ceiling, as if **Zone tools only** were on. It keeps Zone's tools,
  which go through the ceiling as usual.

### Unattended runs

Task runs have nobody to ask: they approve every call themselves. Under a
ceiling, a task run's call above the ceiling is refused at once with "Your
organization's policy requires a person to approve this tool call, and nobody
can approve it in this run." The agent gets the refusal as the tool's result,
and Zone records `agent_policy.tool_denied` with the tool's name.

## MCP servers

`mcp_servers` lists the MCP servers agents get, by name: the names shown in
**Settings → Organization → MCP servers**, and the instance's own servers. A
name is 1 to 32 letters, digits, `-` and `_`; anything else is refused with
`policy_mcp_server_invalid`. Servers not on the list are left out of every
chat and task run, whether they belong to the organization or the instance.

A workspace's own **On** and **Off** settings for organization servers still
apply on top of the policy.

## Network

`domains` restricts where agents may send requests. An entry such as
`example.com` matches that host and every subdomain of it. An entry is a host
name or an IP address, with no scheme, port or path; anything else is refused
with `policy_domain_invalid`. Zone lowercases entries and drops a leading `*.`
and a trailing `.`, so `*.Example.com.` is saved as `example.com`.

When either layer sets `domains`, even to an empty list, every chat and task
run in the workspace starts from what an offline chat gets (see
[Tool HTTP Routing](../SECURITY.md#tool-http-routing)):

- tools that reach the network are withheld, including `web_search`, Zone's
  GitHub tools and stdio MCP servers;
- the web search Zone runs before a turn is skipped;
- coding agents run without their own tools;
- shell commands run under the operating system's network deny, and
  `run_command` refuses `curl` and `wget`.

Zone then lets two things back in:

- **`fetch_url`**, for hosts the policy allows. Zone checks the host before
  the request and again after every redirect, and the usual VPN rules still
  apply. A refused host gets "Your organization's policy doesn't allow
  requests to this domain from this workspace." and is recorded as
  `agent_policy.tool_denied` with the host.
- **HTTP MCP servers** whose URL host the policy allows, and whose name it
  allows when `mcp_servers` is set.

`domains: []` lets nothing back in. A chat that's offline in its own right
gets nothing back either.

### What the network rule doesn't cover

`domains` restricts the agent, not Zone. These keep working whatever it says:

- the chat's model endpoint, which `providers` governs instead;
- Zone's own model infrastructure: embeddings, Ollama, LiteLLM, and ComfyUI
  image, video and audio generation;
- Zone's own GitHub traffic: cloning a project's checkout, tracking pull
  requests, the pull request indicator, auto-fix sweeps and GitHub App calls;
- knowledge and source sync.

Of these, only the GitHub traffic stops for a chat that's offline in its own
right, as it always has.

## What people see

| Where | What happens |
|---|---|
| Chat | The turn doesn't start, and the chat shows the policy's message. Over the WebSocket this is `{"type": "denied", "message": "…", "code": "policy_…"}`. |
| Provider, model and agent pickers | Denied options are disabled, with the reason. API responses carry it as `denial`. Listing a workspace's models never refuses: when the policy denies the workspace's provider or agent, or can't be read, every model carries that denial. |
| REST requests | 403 with `{"error": "…", "code": "policy_…"}`. |
| Task runs | The run fails with the policy's message. `policy_unreadable` is retried; every other denial is final. |
| Tool calls | The tool trace shows the call as refused by policy. |

## Audit log

Zone writes these entries to the organization's audit log.

| Action | When | Resource | Recorded values |
|---|---|---|---|
| `agent_policy.updated` | A policy is saved and its rules changed | `organization_agent_policy` or `workspace_agent_policy`, with the organization's or workspace's ID | The rules before and after |
| `agent_policy.turn_denied` | A chat turn, task run or background completion is refused | `chat`, `task` or `workspace` | `code`, and `detail`: the model, when there is one |
| `agent_policy.tool_denied` | A tool call above the ceiling is refused in an unattended run, or `fetch_url` is refused a host | `chat`, `task` or `workspace` | `code`, and `detail`: the tool's name or the host |

Saving a policy without changing it writes no entry.

## Codes

Refusals of something an agent or a person tried to use, where {provider} and
{agent} are the labels from [Providers](#providers) and
[Coding agents and sign-ins](#coding-agents-and-sign-ins):

| Code | Message |
|---|---|
| `policy_provider_denied` | Your organization's policy doesn't allow {provider} in this workspace. |
| `policy_agent_denied` | Your organization's policy doesn't allow {agent} in this workspace. |
| `policy_model_denied` | Your organization's policy doesn't allow this model in this workspace. Choose an allowed model. |
| `policy_mcp_server_denied` | Your organization's policy doesn't allow this MCP server in this workspace. |
| `policy_domain_denied` | Your organization's policy doesn't allow requests to this domain from this workspace. |
| `policy_approval_required` | Your organization's policy requires a person to approve this tool call, and nobody can approve it in this run. |
| `policy_unreadable` | Your organization's agent policy couldn't be read, so this can't start. Try again shortly. |

Refusals of a policy change:

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | There's no active or grace Team or Enterprise license. See [Licensing](#licensing). |
| `policy_body_invalid` | 400 | The request body can't be read: it isn't valid JSON, has a key that isn't a policy field, or has a value of the wrong type. |
| `policy_wider_than_organization` | 400 | A workspace field would allow more than the organization's. |
| `policy_model_invalid` | 400 | A model pattern is empty, longer than 128 characters, has whitespace, or has a `*` that isn't last. |
| `policy_domain_invalid` | 400 | A domain has a scheme, port or path, or isn't a host name or IP address. |
| `policy_mcp_server_invalid` | 400 | A server name isn't 1 to 32 letters, digits, `-` and `_`. |
| `policy_ceiling_invalid` | 400 | The ceiling is `read` or `outward`. |

## API

| Method and path | Purpose |
|---|---|
| `GET /api/organizations/{org_id}/agent-policy` | The organization's policy and licensing state. |
| `PATCH /api/organizations/{org_id}/agent-policy` | Change the organization's policy. |
| `GET /api/organizations/{org_id}/workspaces/{ws_id}/agent-policy` | A workspace's own policy, its organization's, and the effective result. |
| `PATCH /api/organizations/{org_id}/workspaces/{ws_id}/agent-policy` | Change the workspace's own policy. |

A request is checked in this order: membership (404), the caller's role
(403), the license (403 `license_required`), the body (400
`policy_body_invalid`), then each field (400 with the field's code).

A `PATCH` body carries only the fields you're changing:

- a field left out keeps its value;
- `null` clears it, so this layer no longer restricts it;
- a value replaces it, and `[]` allows nothing.

```http
PATCH /api/organizations/{org_id}/agent-policy
Content-Type: application/json

{"providers": ["bedrock", "vertex"], "agents": [], "ceiling": "write"}
```

`GET` and `PATCH` answer with the same shape. For an organization:

```json
{
  "entitled": true,
  "enforced": true,
  "edition": "enterprise",
  "status": "active",
  "editable": true,
  "policy": {
    "providers": ["bedrock", "vertex"],
    "agents": [],
    "models": null,
    "ceiling": "write",
    "mcp_servers": null,
    "domains": null
  },
  "updated_at": "2026-11-17T09:30:00Z"
}
```

| Field | Meaning |
|---|---|
| `entitled` | The license allows editing: Team or Enterprise, active or in grace. |
| `enforced` | The license enforces policies: Team or Enterprise, in any state. |
| `edition` | `community`, `team` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `editable` | The license allows editing and you may change this policy. |
| `policy` | The organization's rules. All six fields are always present; `null` means unset. |
| `updated_at` | When the policy was last saved, or `null` if it never was. |

A workspace's response has the same licensing fields and `updated_at`, and in
place of `policy`:

| Field | Meaning |
|---|---|
| `organization` | The organization's rules. |
| `workspace` | The workspace's own rules. |
| `effective` | What chats and tasks in the workspace run under: the providers, agents and MCP servers both layers allow; the lower ceiling; and for models and domains, the workspace's entries the organization covers, or the organization's own when the workspace sets none. |
