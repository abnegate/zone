# Zone Configuration Reference

Complete documentation of all configuration options available in `.env`

## 📝 Configuration Philosophy

**Zone requires ZERO configuration to start!**

All variables have working defaults. Keep host Ollama running, then:
```bash
ollama serve
cp .env.example .env
mkdir -p auth && htpasswd -cB auth/users.htpasswd admin
make up
```

For production, regenerate secrets for security.

---

## 🌐 Domain Configuration

### `DOMAIN_HOST_WEBUI`
- **Default**: `webui.localhost`
- **Description**: Shared base domain for service hostnames
- **Example**: `ai.yourdomain.com`
- **Usage**: Configure DNS A record or add to /etc/hosts
- **Note**: Retained variable name for compatibility; Zone chat is at `https://manager.<domain>/chats`

---

## 🔐 Security & Authentication

### `BASICAUTH_REALM`
- **Default**: `"Zone AI Stack"`
- **Description**: Realm name displayed in browser authentication prompt
- **Example**: `"My Private AI"`
- **Note**: Quotes required if contains spaces

### `BASIC_AUTH_USERS_FILE`
- **Default**: `./auth/users.htpasswd`
- **Description**: Path to Apache htpasswd file for basic authentication
- **Generate**: `htpasswd -cB auth/users.htpasswd username`

### `LITELLM_MASTER_KEY`
- **Default**: `dev-insecure-key-change-for-production`
- **Description**: Master API key for authenticating to LiteLLM
- **Security**: **Insecure default** - change for production!
- **Generate**: `openssl rand -base64 32`
- **Usage**: Used by Manager to authenticate to LiteLLM

### `LITELLM_SALT_KEY`
- **Default**: `dev-insecure-salt-change-for-production`
- **Description**: Salt key used for internal hashing
- **Security**: Optional but recommended for production
- **Generate**: `openssl rand -base64 32`

### `SEARXNG_SECRET_KEY`
- **Default**: `dev-insecure-key-change-for-production`
- **Description**: Secret key for session encryption in SearXNG
- **Security**: **Insecure default** - change for production!
- **Generate**: `openssl rand -base64 32`
- **Usage**: Required if using VPN profile (web search)

---

## 🤖 Ollama Model Configuration

### `OLLAMA_MODEL_FAST`
- **Default**: `llama3.1:8b`
- **Description**: Fast model for simple queries and general chat
- **RAM**: ~4-8GB
- **Use for**: Summaries, rewrites, simple questions, translations
- **Options**:
  - `llama3.2:3b` (smaller, faster)
  - `llama3.1:8b` (balanced)
  - `qwen2.5:7b` (alternative)
  - `mistral:7b` (alternative)

### `OLLAMA_MODEL_REASON`
- **Default**: `deepseek-r1:14b`
- **Description**: Reasoning model for complex analysis and deep thinking
- **RAM**: ~8-16GB
- **Use for**: Debugging, system design, proofs, complex math
- **Options**:
  - `deepseek-r1:7b` (smaller, faster reasoning)
  - `deepseek-r1:14b` (balanced)
  - `deepseek-r1:32b` (highest quality reasoning)
  - `llama3.1:70b` (alternative large model)

### `OLLAMA_MODEL_EMBED`
- **Default**: `qwen3-embedding:0.6b`
- **Description**: Embedding model for search, memory recall and RAG. The manager reads it at startup and uses it for every workspace without an embedding model of its own; the bundled Ollama init pulls it.
- **RAM**: ~1-2GB
- **Options**:
  - `qwen3-embedding:0.6b` (default; 1024 dimensions, the width the vector store keeps)
  - `nomic-embed-text` (smaller and faster; 768 dimensions, padded to 1024)
  - `mxbai-embed-large` (1024 dimensions)
- **Note**: Vectors written by one model do not compare with another's, so switching models means re-embedding what is indexed. With `EMBEDDING_ENGINE=local`, name a model the in-process engine can run, such as `nomic-embed-text`.

### `OLLAMA_HOST`
- **Default**: `0.0.0.0:11434`
- **Description**: Bind address for a bundled Ollama container
- **Usage**: Only applies with `--profile bundled-ollama`

### `OLLAMA_BASE_URL`
- **Default**: `http://host.docker.internal:11434`
- **Description**: Where LiteLLM, the manager, and metrics reach Ollama
- **Usage**: Host daemon is the default so Docker Desktop / Apple Silicon can use Metal. For a bundled container, set `http://ollama:11434` and start with `--profile bundled-ollama`.

### `EMBEDDING_ENGINE`
- **Default**: `ollama`
- **Description**: How the manager computes embeddings for search and RAG. The model is always `OLLAMA_MODEL_EMBED`; this only chooses where it runs.
- **Options**:
  - `ollama` — call Ollama over HTTP. Uses the GPU when Ollama has one.
  - `local` — run the model in-process via ONNX Runtime on CPU. No network hop and no contention with the chat model for Ollama's loaded-model slots.
- **When `local` wins**: short texts (chat messages, search queries) and any host where Ollama has no GPU. On a GPU-backed Ollama, long document chunks are still faster over HTTP.
- **First boot**: downloads ~520MB of weights into the `zone_manager_embed_cache` volume before the server binds. `ZONE_EMBED_CACHE_DIR` overrides the location.
- **Build requirement**: needs a `zone-server` built with the `local-embeddings` Cargo feature, which is on by default. ONNX Runtime publishes no musl builds, so the manager image is Debian-based rather than Alpine.

---

## 🧑‍💻 Model Backend

Where chat turns and task runs get their completions, along with chat titles,
pull request subjects, and auto-project reviews and summaries. The default is
the OpenAI-compatible endpoint `LITELLM_HOST` names. An organization can instead
choose a coding agent CLI as its provider, **Claude Code** or **Codex**, and
sign it in with its own Claude or ChatGPT subscription. Zone then runs that CLI
for the organization's completions and serves Zone's tools to it over MCP. The
manager image ships both CLIs: claude 2.1.278 and codex 0.156.1.

### Choosing a provider

Organization admins and owners choose the provider under **Organization
Settings > AI Settings**, where *Claude Code (Claude subscription)* and *Codex
(ChatGPT subscription)* sit beside Self-Hosted, OpenAI, Anthropic and AWS
Bedrock (`claude_code` and `codex` in the API). A workspace admin can choose
one for a single workspace under **Workspace Settings > AI Settings**, with
**Override organization AI settings** on; the workspace then runs on its
organization's sign-in for that agent. Organizations and workspaces that choose
neither follow `ZONE_LLM_BACKEND`, the instance-wide default.

No instance-wide setting turns these providers off: any organization admin can
select them. Read *Security* below before using them on an instance shared by
organizations that must not see each other's data.

### `ZONE_LLM_BACKEND`
- **Default**: `litellm`
- **Description**: The instance-wide default, for organizations and workspaces
  whose provider is not Claude Code or Codex
- **Options**:
  - `litellm` (the endpoint `LITELLM_HOST` names)
  - `claude` (runs the `claude` CLI)
  - `codex` (runs the `codex` CLI)
- **Note**: With `claude` or `codex`, `LITELLM_HOST` and `LITELLM_KEY` are no
  longer required at boot, so a host with no LiteLLM at all can start. The CLI
  then always uses the login of the user the server runs as, whatever
  `ZONE_AGENT_HOST_LOGIN` says: Zone never refuses these turns as not signed
  in, and without a login the CLI fails them in its own words. claude still
  gets the variables and flags under *How a turn runs*, and codex
  `ZONE_CODEX_SANDBOX`, but neither gets an organization's home, and both run
  in the server's own working directory rather than an organization's. In the
  manager image that directory is the root-owned `/app`, so an agent given its
  own tools cannot write there. Zone signs no one in for this path; in the
  compose stack, choose Claude Code or Codex in AI settings instead.

### `ZONE_LLM_BACKEND_EXECUTABLE`
- **Default**: unset, so the agent's own name is looked up on `PATH`
  (`/usr/local/bin` in the manager image)
- **Description**: An explicit binary to run instead, for a CLI that is not on
  the server's `PATH` (`/opt/homebrew/bin/codex`, say)
- **Usage**: Only with `ZONE_LLM_BACKEND` set to `claude` or `codex`; setting it
  otherwise is refused at boot, because it would mean believing a CLI was
  serving turns while the HTTP endpoint was still being billed. It applies to
  that agent wherever Zone runs it, for organizations that choose it as their
  provider too; the other agent is still looked up on `PATH`. Compose does not
  pass it.

### `ZONE_AGENT_STATE_DIR`
- **Default**: `$XDG_STATE_HOME/zone/agents`, falling back to
  `$HOME/.local/state/zone/agents`
- **Compose and Helm**: `/app/agent-state`, on the `zone_manager_agent_state`
  volume in compose
- **Description**: Where each organization's CLI keeps its state.
  `<dir>/<organization id>/claude` is claude's `CLAUDE_CONFIG_DIR` and
  `<dir>/<organization id>/codex` is codex's `CODEX_HOME`. Each has a `work`
  directory, where every turn of that agent for that organization runs. Zone
  creates the organization's directories with mode 0700 and leaves the mode of
  an existing root alone.
- **Contents**: codex's login (`auth.json`, which codex renews itself) and both
  CLIs' session transcripts. Zone keeps the Claude token in the database and
  hands it to each turn in `CLAUDE_CODE_OAUTH_TOKEN`; it does not write it
  here. Zone does not prune the transcripts. Deleting an organization stops
  any codex sign-in it has in progress, runs `codex logout` in its codex home,
  and removes `<dir>/<organization id>` with everything in it.
- **Note**: Must be an absolute path. A relative one is refused at boot, and so
  is an unset one when neither `XDG_STATE_HOME` nor `HOME` is absolute.

### `ZONE_AGENT_HOST_LOGIN`
- **Default**: `true`. Compose sets `false` unless `.env` says otherwise, and so
  does the Helm chart, in `server.env`.
- **Description**: Whether an organization that chose Claude Code or Codex but
  has not signed in may use the login of the user the server runs as. See
  *Running Zone natively* below. It does not apply to `ZONE_LLM_BACKEND`
  set to `claude` or `codex`, which always runs on that login.
- **Options**: `true`, `1`, `yes` or `on`, and `false`, `0`, `no` or `off`, in
  any case. Anything else is refused at boot.
- **Note**: In a container that would be a login made inside the container,
  answering for every organization, which is why compose and Helm turn it off.
  Turn it off on a native server shared by several organizations too.

### `ZONE_CLAUDE_TOKEN_URL`
- **Default**: `https://platform.claude.com/v1/oauth/token`
- **Description**: Where Zone exchanges a Claude authorization code for tokens
  and renews them. Tests point it at a local mock. Compose passes it from
  `.env`, where an empty value keeps the default.
- **Note**: Must be an absolute `http` or `https` URL with a host, carrying no
  credentials, query or fragment. Anything else is refused at boot.

### `ZONE_CODEX_SANDBOX`
- **Default**: `workspace-write`. The manager image sets `danger-full-access`,
  which compose and the Helm chart keep.
- **Description**: The sandbox codex runs its own tools in on a turn that grants
  them, that is with **Zone tools only** off. A turn that withholds them runs
  `read-only` whatever this says.
- **Options**:
  - `workspace-write` (codex confines its shell with bubblewrap)
  - `danger-full-access` (no sandbox: codex's shell runs as the server's user,
    with all of that user's file and network access)
- **Note**: bubblewrap needs user namespaces, and Docker's default seccomp
  profile blocks them, so in a container every sandboxed command fails with
  `bwrap: No permissions to create a new namespace`. Set `workspace-write`
  where your container runtime allows user namespaces. Any other value is
  refused at boot.

### `ZONE_AGENT_ENV_PASSTHROUGH`
- **Default**: empty
- **Description**: Extra variables, comma separated, that the claude and codex
  CLIs, and Zone's own chat and task tools, may inherit from the server's
  environment
- **Note**: A CLI starts from an empty environment and inherits only `HOME`,
  `PATH`, `USER`, `LOGNAME`, `SHELL`, `TERM`, `LANG`, `LC_*`, `TZ`, `TMPDIR`,
  `XDG_RUNTIME_DIR`, the proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`,
  `NO_PROXY` and `ALL_PROXY`, in upper or lower case), `SSL_CERT_FILE`,
  `SSL_CERT_DIR`, `NODE_EXTRA_CA_CERTS` and the names listed here. Unless named
  here, the database URL, the JWT and encryption keys, `LITELLM_KEY`,
  `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME`
  stay behind. Zone then adds what the turn needs; see *How a turn runs*.
- **Behaviour change**: A CLI used to inherit the server's whole environment
  apart from the provider API keys. A setup that relied on that, such as an
  `ANTHROPIC_BASE_URL` in the server's environment, now has to name the
  variable here. What is named reaches every organization's CLI and the tools
  of every chat and task, so name nothing secret. In compose the manager
  container receives only the variables `docker-compose.yml` lists, so add the
  variable to the manager service's `environment` as well.

### `ZONE_CHAT_AGENT_CWD`
- **Default**: the server's working directory. Compose and the Helm chart set
  `/app/workspace`, and the `dev` profile `/app/runner`.
- **Description**: Where Zone's own chat tools start: relative paths given to
  `read_file`, `run_shell` and the other host tools resolve against it, and
  background jobs keep their logs in its `.zone/jobs`. Every organization's
  chats share it. It is not where a CLI runs: on Claude Code or Codex, that is
  the organization's `work` directory.
- **Note**: `/app` is root-owned in the manager image, so the tools need a
  directory the `zone` user can write, and `/app/workspace` is one.

### Signing in

Each organization signs in to each agent once, in the panel that appears under
the provider on **Organization Settings > AI Settings**; a workspace's AI
override shows the same panel. Only organization admins and owners can sign in
or out. Other members see the status, with "Ask an organization admin to sign
in" while the agent is not signed in.

**Claude Code** uses the sign-in `claude setup-token` uses:

1. **Sign in with Claude** gives you a link to claude.com. Sign in there and
   approve access.
2. claude.com shows a code. Paste it, or the address of the page showing it,
   into **Code from claude.com** and submit it. The admin who started has to do
   this within ten minutes. The panel shows until when, and drops the link
   once that time has passed.
3. Zone exchanges the code at `ZONE_CLAUDE_TOKEN_URL` and stores the tokens in
   the database, sealed with a key derived from `ENCRYPTION_KEY`. The panel
   shows the plan, such as Claude Team, when the token response names one. It
   shows an expiry date only for a sign-in Zone cannot renew, one whose token
   came without a refresh token.

Zone asks for inference access only, with a one-year lifetime, as
`claude setup-token` does. If claude.com refuses that on its page, **Try again
with full access**, which the panel shows beside the code field, starts over
with the wider set of scopes claude's own login asks for, without the one-year
lifetime. The panel leaves that button out when the sign-in already asks for
full access.

A paste Zone cannot read, such as a code with its `#state` cut off, leaves the
sign-in open, so you can paste again. Any other failure ends it: claude.com
rejecting the code, or a sign-in that expired or was already used. The panel
then drops the link and offers **Start again**, which starts over with the same
access, beside **Try again with full access**. A sign-in in progress survives
switching to another settings tab and back.

When a refresh token came with it, Zone renews a Claude token before handing
it to a turn if the token would expire within the hour, the longest a task
attempt runs. A task run takes the token afresh for each attempt. Concurrent
turns renew it once, and a renewal that fails before the token expires leaves
the current token in use.

**Codex** uses codex's own device-code sign-in:

1. **Sign in with ChatGPT** runs `codex login --device-auth` on the server.
2. The panel shows OpenAI's link, `https://auth.openai.com/codex/device`, and a
   one-time code that expires after 15 minutes. Open the link, sign in to
   ChatGPT and enter the code. The panel checks every three seconds, backing
   off while checks fail, and shows Signed in once codex has saved the login.
   It stops showing the code once the code has expired.

The one-time code is shown only to organization admins and owners and to
whoever started the sign-in. codex signs in inside a staging directory, and
its new `auth.json` replaces the organization's only when the sign-in
succeeds, so a refused or expired attempt leaves an existing login as it was.
**Cancel** stops the sign-in by signing the organization out of codex. If
OpenAI refuses to issue a code, the panel shows codex's error, such as
`device code request failed with status 403 Forbidden`: codex reports the HTTP
status OpenAI answered with, not the body of the answer. A sign-in that fails
on the server's own disk, such as a state directory it cannot write, shows
only an internal error; the server's log has the details.

**Signed in means the credentials are there.** Neither CLI checks a login when
asked for its status: `claude auth status` reports one for any token it finds,
and `codex login status` reads `auth.json` without calling OpenAI. Zone does
not check either. Claude Code shows Signed in while Zone holds a token that it
can open with the current `ENCRYPTION_KEY` and that has not expired or can be
renewed; a token sealed under another key shows as expired. Codex shows Signed
in while the organization's `auth.json` exists. A login revoked upstream, or
one codex can no longer renew, shows up on the next turn instead: the turn
fails in the CLI's own words, followed by "Sign in again under Organization
Settings > AI Settings." A task run that fails that way stops without spending
its retries, since a retry signs no one in.

**Signing out** deletes the organization's Claude tokens from Zone; it does not
revoke them with Anthropic. For Codex it stops a sign-in in progress and runs
`codex logout`, which asks OpenAI to revoke the login and deletes `auth.json`.
The panel asks before it signs out, since the sign-out applies to every
workspace of the organization. Sign-ins and sign-outs are recorded in the
organization's audit log as `agent.signed_in` and `agent.signed_out`. Deleting
the organization deletes its Claude tokens with it and signs it out of codex
the same way.

The panel uses these routes, where `{agent}` is `claude` or `codex`:

| Route | Who | What |
|-------|-----|------|
| `GET /api/organizations/{org_id}/agents` | Any member | Both agents' status and models |
| `GET /api/organizations/{org_id}/agents/{agent}` | Any member | One agent's status |
| `POST /api/organizations/{org_id}/agents/{agent}/login` | Admins and owners | Start a sign-in; `{"scope":"full"}` asks claude for full access |
| `POST /api/organizations/{org_id}/agents/claude/login/code` | The admin who started | Finish a Claude sign-in with `{"code":"..."}`; a refusal's `kind` is `invalid_code` (paste again) or `start_again` |
| `DELETE /api/organizations/{org_id}/agents/{agent}/login` | Admins and owners | Sign out |

### How a turn runs

For an organization on Claude Code or Codex, each chat turn, task run, title,
review or summary starts the CLI where the server runs:

- in the organization's `work` directory under `ZONE_AGENT_STATE_DIR`;
- with the environment described under `ZONE_AGENT_ENV_PASSTHROUGH`, plus the
  organization's home (`CLAUDE_CONFIG_DIR` or `CODEX_HOME`) and, for claude,
  the token in `CLAUDE_CODE_OAUTH_TOKEN`;
- for claude, with `DISABLE_AUTOUPDATER=1` and
  `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` (no updates, telemetry or error
  reports), `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` (Zone keeps its own memory),
  and `MCP_TOOL_TIMEOUT` and `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT` at 30 minutes;
- for claude, with `--setting-sources ""`, so it loads no settings file, hook,
  `CLAUDE.md`, skill, agent or command from its home, its working directory or
  any directory above it, and with
  `--settings '{"crossSessionInbound":"refuse"}'`, a setting meant to refuse
  messages from other claude sessions of the same OS user.

An organization that chose Claude Code or Codex without signing in, where the
host-login fallback is off, gets "The claude CLI is not signed in for this
organization. An organization admin can sign in under Organization Settings >
AI Settings." (or the same for codex) instead of an answer.

In the manager image both binaries are pinned by SHA-256 and live in
`/usr/local/bin`, with codex's bubblewrap at
`/usr/local/bin/codex-resources/bwrap`. They are root-owned, so a turn cannot
replace them, and claude's auto-updater is off.

### What the agent can reach

Zone's own tools are offered to the agent over MCP, to claude and codex alike:
Zone serves them at `/mcp` on its own port for the life of one turn, behind a
bearer token minted for that turn and revoked when it ends. The token reaches
the CLI in the `ZONE_MCP_TOKEN` variable, never on its command line. The agent
calls the tools as `mcp__zone__<name>`, Zone executes them, and every call goes
through the approval policy a chat tool call goes through: a chat with
Auto-approve off raises the usual card and waits for you, and a denial refuses
the call. So retrieval, the workspace tools and citations work on these turns,
and the console shows what the agent did the way it always does. A task run
gets its task tools the same way, and approves every call, as task runs
always do. Its agent is not offered the tools that end Zone's own turn to
wait, `ask_user`, `wait_for` and `submit_plan`, and a call to one is refused
as an unknown tool: over MCP such a call returns at once with nothing waiting
behind it, so the run would finish on a question never asked or a job never
waited for. A chat turn's agent is offered its whole registry.

codex is told to pass every call to Zone without asking, because headless
codex has no one to ask and Zone applies the approval policy itself; to fail
the turn at startup when it cannot reach Zone's endpoint; and to list Zone's
tools to the model up front. Both CLIs wait up to 30 minutes for a tool call.
codex sends no cancellation when it stops waiting, so a shorter limit would
leave an approval card open for a call codex had already given up on.

One thing differs from a turn served by the endpoint: the agent runs its own
loop rather than Zone's, so Zone takes one round and the agent decides for
itself how many tool calls it makes inside that round. A chat with **Agent
mode** off offers no tools: the CLI runs with its own tools withheld and
without Zone's.

### The agent's own tools

A chat with Agent mode on carries a **Zone tools only** toggle, on by default,
beside Auto-approve. On, the agent is confined to the tools Zone serves it:

- claude runs with `--tools ""`: none of its built-in tools;
- codex runs with `--sandbox read-only`, and with its shell and its other
  built-in tools switched off. The one built-in it cannot switch off,
  `apply_patch`, is refused by the read-only sandbox.

Either way, claude runs with `--strict-mcp-config`, so no MCP server but Zone's
loads, and codex with `--ignore-user-config`, `--disable apps` and
`--disable plugins`, so neither a `config.toml` nor ChatGPT's apps and plugins
add tools beside Zone's.

Off, the agent also keeps the tools it ships with. They run inside the CLI's
process as the user the server runs as, where Zone can neither show them to you
nor approve them. Zone passes neither CLI a flag that skips its permission
prompts, so claude applies its own rules to them. Headless codex never asks:
its shell runs whatever `ZONE_CODEX_SANDBOX` allows, which in the manager image
is everything the server's user can do.

The two toggles are independent on purpose. Auto-approve decides whether Zone's
tools run without asking; Zone tools only decides whether there are tools Zone
never sees at all. Turning both off their safe settings on a single-user
self-host is allowed, and means what it says: a chat message can read, write
and run commands where the server runs, with nothing standing in between.

### Naming a model

With Claude Code or Codex as the provider, the Fast and Reasoning fields in AI
settings list that agent's own models, and Automatic leaves the choice to the
agent:

- Claude Code: `sonnet`, `opus` and `haiku`, the aliases claude resolves to its
  latest models.
- Codex: `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-sol`,
  `gpt-5.6-terra`, `gpt-5.6-luna` and `gpt-5.5`, the presets codex 0.156.1
  ships, in its order. A signed-in ChatGPT account may see a different set.

Zone passes a model to the CLI as `--model` only when that agent knows the
name, and otherwise passes none, so the agent uses its own default. claude
knows its aliases in any case (`sonnet`, `opus`, `haiku`, `fable`, `best`,
`opusplan`, `sonnet[1m]`, `opus[1m]` and `fable[1m]`) and lowercase full names
such as `claude-opus-4-8`, optionally ending in `[1m]`. codex knows its presets
and lowercase names beginning `gpt-`. Neither knows an empty name, `auto`, or
a name containing a colon or a space, so an Ollama model such as `llama3.2:3b`
or `gpt-oss:20b` never reaches an agent. `fable` is known but not offered: a
Pro or Max subscriber has to accept its usage credits interactively first, and
a headless turn without that falls back to another model or fails.

A turn asks for the chat's own model only when the agent knows it. Otherwise
it asks for the Reasoning model from AI settings when Zone judges the prompt
needs reasoning, and for the Fast model when not, provided the agent knows the
name; with no known model to ask for, it lets the agent choose. A task run
chooses the same way from the task's own model, so on these providers it
starts even when no model is installed or configured.

Chat titles, pull request subjects and auto-project summaries use the Fast
model, so on Claude Code or Codex they need one the agent knows. Without one, a
chat keeps the title it was created with, a pull request subject is made from
the task's title, and an auto-project summary is the pull request's first
paragraph. Search and retrieval keep using the server's own embedding engine
(`EMBEDDING_ENGINE` and `OLLAMA_MODEL_EMBED`).

### Reviews, conflict repair and plan approval

- **Auto-project reviews** run on the workspace's CLI without Zone's review
  tools, such as `read_pr_file`. The reviewer judges the task, the pull request
  and the diff in its prompt, and its instructions leave the tools out. When
  the workspace's CLI cannot be used, because it is signed out for example,
  the auto project pauses with that message. The reviewer's model is one the
  agent knows, taken from `ZONE_AUTO_REVIEW_MODELS` and AI settings, or else
  one of the agent's own models, never an installed Ollama model. A run whose
  agent chose its own model records no model, so with
  `ZONE_AUTO_REVIEW_REQUIRE_DISTINCT_MODEL` on its review pauses unless a
  review bot answers; set Fast and Reasoning models the agent knows to avoid
  that.
- **Conflict repair** runs Zone's own tool loop, which a CLI cannot host, so it
  always runs on the instance's LiteLLM endpoint, whatever the workspace chose.
  With `ZONE_LLM_BACKEND` set to `claude` or `codex` the instance has no such
  endpoint, and repair fails with "Conflict repair needs zone's own tool loop,
  which a coding agent CLI backend does not provide".
- **Plan approval** is not available. A task run that requires it fails before
  the CLI starts, without a retry, with "Plan approval is not available when a
  task runs on a coding agent CLI; turn off Require plan approval or use the
  Self-Hosted provider." A run an auto project starts is never held for
  approval, so it is unaffected.

### Running Zone natively: the host login

When Zone runs as a native process and `ZONE_AGENT_HOST_LOGIN` is on, which it
is by default, an organization that chose Claude Code or Codex but has not
signed in falls back to the CLI login of the user the server runs as. The panel
then says "Using this server's own Claude Code sign-in" (or Codex), with the
plan or login type the CLI reports; Zone reads it from `claude auth status` or
`codex login status`, and names a Claude plan the way it names a Zone
sign-in's, such as Claude Team. Every organization without a sign-in of its
own shares that login, which is why a server with several organizations
should turn the fallback off.

The fallback runs the CLI without `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, which
the environment allowlist drops, so each CLI uses its default home under the
server user's `HOME`: `~/.claude` and `~/.codex`. The turn still runs in the
organization's `work` directory, but the CLI writes its session transcripts
into those default homes, beside the user's own.

- **A login made with `CLAUDE_CONFIG_DIR` set is not found.** On macOS claude
  keeps such a login in a separate Keychain item keyed to that directory, so a
  shell whose `claude auth status` reports a login can still leave the
  fallback with none. Sign in without the variable
  (`env -u CLAUDE_CONFIG_DIR claude auth login`), or name `CLAUDE_CONFIG_DIR`
  in `ZONE_AGENT_ENV_PASSTHROUGH` so the fallback uses that directory.
- **The host's settings are not used.** Every claude turn runs with
  `--setting-sources ""`, so the fallback takes the host's login but not its
  `~/.claude/settings.json` (its `env` block, `apiKeyHelper` or model), hooks,
  `CLAUDE.md` files, skills, agents or commands. codex runs with
  `--ignore-user-config`, which ignores `~/.codex/config.toml` but still reads
  the login from `~/.codex`.

With the fallback off, as in compose and Helm, such an organization gets the
not-signed-in error above.

### Security: every organization is the same OS user

Every organization's CLI runs as the same operating-system user as the server:
`zone` in the manager image, and uid 1000 in the Helm chart's pods by default.
Nothing at the OS level separates one organization's agent state from
another's, so any process running as that user can:

- read every organization's agent state: codex's `auth.json`, which is a
  working ChatGPT login, and both CLIs' session transcripts;
- read the `/proc/<pid>/environ` of any CLI running at the time, which holds
  that turn's `CLAUDE_CODE_OAUTH_TOKEN` and, when Zone serves it tools, its
  `ZONE_MCP_TOKEN`. The second lets its holder call that turn's Zone tools, as
  that turn's user and under its approval policy, until the turn ends;
- write into any organization's agent state: replace or delete its codex
  login, or plant files for its CLI to read;
- reach whatever the server can reach on the network. In the compose stack
  that includes Valkey, which has no password there, and a host Ollama at
  `host.docker.internal`.

The processes that run as that user are:

- each CLI, and on a turn with **Zone tools only** off, its own shell and file
  tools;
- Zone's own chat tools, whatever the provider. With Agent mode on,
  `read_file`, `list_files` and `search_code` reach any path the server's user
  can read, other than the agent state and the `/proc` entries described below,
  without an approval card. `write_file` and `apply_patch` do the same once
  approved or with Auto-approve on, and `run_shell` and `run_command` then
  reach everything above;
- configured stdio MCP servers and `COMFYUI_TRAIN_COMMAND`, which also inherit
  the server's full environment.

What stands in the way:

- On Linux the server makes itself non-dumpable, so no process of its user can
  read the server's own environment, which holds the database URL, the JWT and
  encryption keys and the LiteLLM key. That protects the server process only.
  The CLIs, MCP servers and training commands it starts can be read as above,
  and the last two carry that same full environment.
- Zone's file tools, `read_file`, `list_files`, `search_code`, `write_file`
  and `apply_patch`, refuse any path under the agent state directory and any
  process's `/proc/<pid>` entry, `/proc/self` included, whatever the chat's
  provider or approval setting. The path is resolved first, so `..`, a symlink
  or a link under a `/proc` entry does not get around the refusal, and a
  recursive listing or search leaves those paths out. The check runs just
  before the file is opened, so a process swapping a symlink into the path in
  between would get past it, but a process that can do that as the server's
  user can read the state itself. `run_shell` and `run_command` are not held to
  it.
- `--setting-sources ""`, with `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, stops claude
  from loading settings, hooks, `CLAUDE.md` files or memory planted in its
  home, its working directory or any directory above it, `/app/agent-state`
  included. It cannot make those files unwritable. codex may read an
  `AGENTS.md` planted in its `CODEX_HOME`.
- Each claude session opens a messaging socket under
  `$XDG_RUNTIME_DIR/cc-socks` or `/tmp/cc-socks`, through which, going by
  claude's own code, other sessions of the same OS user can reach it. Zone
  passes every claude turn `--settings '{"crossSessionInbound":"refuse"}'`,
  which claude 2.1.278 accepts; that it keeps two live sessions apart has not
  been tested.
- The environment allowlist, the per-organization homes and working
  directories, and `ZONE_AGENT_HOST_LOGIN=false` in compose and Helm. `/app`
  and every binary in the image are root-owned, so neither the server's user
  nor an agent can replace them. The MCP token travels in a variable, never on
  a command line, and dies with its turn. The Claude token stays sealed in the
  database until a turn needs it.
- The flags under *The agent's own tools*, so no operator configuration,
  ChatGPT app or plugin adds tools beside Zone's. codex still exports its own
  metrics to `ab.chatgpt.com`, and nothing in Zone turns that off.

These providers suit an instance whose organizations trust each other, such
as a personal or single-team compose stack. On an instance shared by
organizations that must not reach each other's data, leave them unselected:
per-organization OS users, which would be the fix, do not exist yet, and
nothing turns the providers off for the whole instance. With or without them,
a chat's `run_shell` and `run_command` reach every organization's agent state
once approved, and its file tools read whatever else the server's user can.

### Operations

- **Compose.** The manager service mounts the `zone_manager_agent_state` volume
  at `/app/agent-state` and passes `ZONE_AGENT_STATE_DIR`,
  `ZONE_AGENT_HOST_LOGIN` (default `false`), `ZONE_LLM_BACKEND` (default
  `litellm`), `ZONE_CHAT_AGENT_CWD` (default `/app/workspace`),
  `ZONE_AGENT_ENV_PASSTHROUGH`, `ZONE_CODEX_SANDBOX` (default
  `danger-full-access`) and `ZONE_CLAUDE_TOKEN_URL` (empty by default, which
  keeps Claude's own endpoint). The image's entrypoint hands
  `/app/agent-state` to `zone` with mode 0700, then runs the server as `zone`.
  The `dev` profile's image, `manager/Dockerfile.dev`, does not include the
  CLIs.
- **Helm.** Agent providers need `server.replicaCount: 1` and
  `server.autoscaling.enabled: false`; see
  [helm/zone-apps/README.md](../helm/zone-apps/README.md).
- **Backups.** `make backup` and `make restore` include
  `zone_manager_agent_state`, and with it every organization's codex login; see
  [OPERATIONS.md](OPERATIONS.md).
- **Upgrading.** This version adds migration 048, after which an older image
  refuses to start against the database with `VersionMissing(48)`. Back up
  first; see [OPERATIONS.md](OPERATIONS.md).
- **Refused at boot**: a relative `ZONE_AGENT_STATE_DIR`, or none when neither
  `XDG_STATE_HOME` nor `HOME` is absolute; a `ZONE_AGENT_HOST_LOGIN` that is not
  one of the spellings above; a `ZONE_CLAUDE_TOKEN_URL` that is not an absolute
  `http` or `https` URL with a host, or that carries credentials, a query or a
  fragment; a `ZONE_CODEX_SANDBOX` other than `workspace-write` or
  `danger-full-access`. Nothing is created in the state directory at boot, so
  one the server cannot write shows up when a turn first needs it, as "Could
  not prepare the claude CLI's state directory: …".

### Known gaps

- A workspace whose AI override chooses Self-Hosted and nothing else, with no
  keys or models of its own, runs on Self-Hosted, but Workspace Settings shows
  it as inheriting its organization's settings.
- A chat's model picker lists only installed Ollama models, so the console
  cannot give a chat `opus` or `gpt-6-sol`; set the Fast and Reasoning models
  in AI settings instead. Whether a chat offers Agent mode also follows those
  installed models.
- A task run on Claude Code or Codex is not offered `ask_user` or `wait_for`,
  but its instructions still describe them, so its agent may call one and be
  refused.

---

## 🎨 ComfyUI Image, Video, and Audio Generation

See [COMFYUI.md](COMFYUI.md) for model setup, hardware requirements, checksum
details, and native macOS / bundled NVIDIA instructions.

### `COMFYUI_ENABLED`
- **Default**: `false`
- **Description**: Enables automatic image-, video-, and audio-intent routing
  and direct ComfyUI generation
- **Set to `true`** only after the runtime and verified checkpoint are ready

### `COMFYUI_BASE_URL`
- **Default**: `http://host.docker.internal:8188`
- **Description**: ComfyUI endpoint used by the manager
- **Native macOS**: Keep the default while ComfyUI runs on the host
- **Bundled NVIDIA**: Set to `http://comfyui:8188` and start the
  `bundled-comfyui` profile
- **Security**: ComfyUI is unauthenticated. Do not use a public URL; the bundled
  service is intentionally confined to the private Compose network.

### `COMFYUI_WORKFLOW_PATH`
- **Default**: `/app/comfyui/workflows/flux1-schnell-fp8-api.json`
- **Description**: In-container path to a workflow file. Graphs in that
  directory overlay packaged copies of the same filename. If
  `../recipes/catalog.json` exists beside that directory, it replaces the
  packaged recipe catalog. Chat still only writes prompt, seed, checkpoint,
  and optional source image.
- **Usage**: The Compose file mounts the repository `comfyui/workflows` and
  `comfyui/recipes` directories here.

### `COMFYUI_CHECKPOINT`
- **Default**: `flux1-schnell-fp8.safetensors`
- **Description**: Fallback ComfyUI checkpoint when org/workspace AI settings
  do not set `model_image`. Path separators and traversal are rejected.
  Chat image generation uses the effective `model_image` setting when present.
  The filename may also be a LoRA in `models/loras/`; the matching adapter
  recipe is selected from its coherent `.zone.json` sidecar. A missing,
  incomplete, or mismatched adapter sidecar fails closed.

### `COMFYUI_MODELS_DIR`
- **Default**: `/app/comfyui/models`
- **Description**: ComfyUI models root scanned for checkpoints, diffusion
  models, and LoRAs. HuggingFace adapter installs and Train output land in
  `loras/` under this directory.

### `COMFYUI_TRAIN_COMMAND`
- **Default**: empty (uses packaged `ZoneTrainLoRA` on the configured ComfyUI)
- **Description**: Optional shell command used by the Models Train tab. When
  empty and `COMFYUI_ENABLED` is true, Zone posts a `ZoneTrainLoRA` graph to
  ComfyUI. The command still receives `ZONE_TRAIN_NAME`, `ZONE_TRAIN_BASE`,
  `ZONE_TRAIN_ATTEMPT`, `ZONE_TRAIN_DIR`, `ZONE_TRAIN_OUTPUT`, `ZONE_TRAIN_TRIGGER`,
  `ZONE_TRAIN_ARCHITECTURE`, `ZONE_TRAIN_FOLDER`, `ZONE_TRAIN_ARTIFACT`, and
  `COMFYUI_BASE_URL` if you override it. `ZONE_TRAIN_NAME` remains the
  user-visible final filename. `ZONE_TRAIN_ATTEMPT`, `ZONE_TRAIN_FOLDER`, and
  `ZONE_TRAIN_ARTIFACT` identify
  isolated runtime namespaces. FLUX runs also receive `ZONE_TRAIN_CHECKPOINT`;
  Qwen edit runs receive
  `ZONE_TRAIN_UNET`, `ZONE_TRAIN_CLIP`, and `ZONE_TRAIN_VAE`. These are resolved
  from explicit recipe training metadata, never inferred from a recipe name or
  the global checkpoint. The command must write the LoRA to
  `ZONE_TRAIN_OUTPUT`. Defaults live in
  `comfyui/custom_nodes/zone_lora/train_config.json` (rank 32, alpha=rank,
  transformer linear layers, 512px, at least 150 steps). Qwen needs paired
  same-index `targets/NNNN.png` and `control_1/NNNN.png` files, with the edit
  instruction in `targets/NNNN.txt`. Its quality score is measured but
  uncalibrated; FLUX health bands do not apply, and live identity retention is
  not yet proven. A command that uses the packaged Python driver should also
  receive `ZONE_TRAIN_DEFER_CLEANUP=1`; Zone performs quality selection and
  cleans that exact UUID namespace afterward. macOS apply:
  `./scripts/setup-comfyui-macos.sh --apply-nodes`. The NVIDIA image copies the
  same folder; Compose bind-mounts it over the container custom node.

### `COMFYUI_CAPTION_MODEL`
- **Default**: empty (auto-captioning disabled)
- **Description**: Vision model used to caption LoRA training images that have
  no caption. Zone first asks the model to name the subject shared by every
  image, then describes each image while excluding that subject, so the trigger
  word carries the identity. Captions typed by hand are never overwritten. Must
  be a model that accepts images; leave empty to caption manually.

### `COMFYUI_CAPTION_TIMEOUT_SECS`
- **Default**: `60`
- **Description**: Per-request budget for one captioning call. A timeout leaves
  the caption as the trigger word alone rather than failing the training job.

### `COMFYUI_TRAIN_TIMEOUT_SECS`
- **Default**: `3600`
- **Description**: Wall-clock budget for a ComfyUI train job.

### `COMFYUI_CLASSIFIER_MODEL`
- **Default**: empty (`auto`)
- **Description**: Optional Fast LiteLLM model used when image-intent rules are
  unsure, including informal edits of an attached photo (`IMAGE` vs `CHAT`,
  3-token reply). Org/workspace Fast overrides this when set. When empty, Zone
  uses the current chat model or a small installed completion model.
- **Timeout**: `COMFYUI_CLASSIFIER_TIMEOUT_SECS` (default `3`, range 1–30).
  Timeouts fall back to normal chat.

### `COMFYUI_VIDEO_WORKFLOW_PATH`
- **Default**: `/app/comfyui/workflows/wan2.2-ti2v-5b-api.json`
- **Description**: In-container path to the versioned Wan 2.2 TI2V
  text-to-video API workflow. Image-to-video uses the sibling file
  `wan2.2-ti2v-5b-i2v-api.json` in the same directory.

### `COMFYUI_VIDEO_UNET`
- **Default**: `wan2.2_ti2v_5B_fp16.safetensors`
- **Description**: Fallback Wan UNET when org/workspace AI settings do not set
  `model_video`. Path separators and traversal are rejected. Chat video
  generation uses the effective `model_video` setting when present.

### `COMFYUI_VIDEO_CLIP`
- **Default**: `umt5_xxl_fp8_e4m3fn_scaled.safetensors`
- **Description**: Text encoder loaded with the Wan video workflow

### `COMFYUI_VIDEO_VAE`
- **Default**: `wan2.2_vae.safetensors`
- **Description**: VAE loaded with the Wan video workflow

### `COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS`
- **Default**: `600`
- **Description**: Wall-clock timeout for a single video generation job

### `COMFYUI_AUDIO_WORKFLOW_PATH`
- **Default**: `/app/comfyui/workflows/ace-step-v1-3.5b-api.json`
- **Description**: In-container path to the versioned ACE-Step v1 3.5B
  text-to-audio API workflow. When the file is missing, the manager falls back
  to the graph compiled into the binary.

### `COMFYUI_AUDIO_CHECKPOINT`
- **Default**: `ace_step_v1_3.5b.safetensors`
- **Description**: Fallback ACE-Step checkpoint when org/workspace AI settings
  do not set `model_audio`. Path separators and traversal are rejected. Chat
  audio generation uses the effective `model_audio` setting when present.

### `COMFYUI_AUDIO_GENERATION_TIMEOUT_SECS`
- **Default**: `600` (range 10–3600, clamped)
- **Description**: Wall-clock timeout for a single audio generation job

### `COMFYUI_COMMIT`
- **Default**: `30bdda1ef13a3a34fce2cd2fec633f15d832122a`
- **Description**: Immutable upstream ComfyUI revision used by the NVIDIA image
- **Recommendation**: Change only together with a reviewed dependency and
  workflow compatibility update

---

## 🔍 Web Search Configuration

### `SEARCH_ENABLE_WEB_SEARCH`
- **Default**: `true`
- **Description**: Enable Zone chat web search through SearXNG
- **Note**: Requires the VPN profile

### `SEARCH_RESULT_COUNT`
- **Default**: `5`
- **Description**: Number of search results supplied to chat

### `SEARCH_SEARXNG_QUERY_URL`
- **Default**: `"http://gluetun:8080/search?q=<query>&format=json"`
- **Description**: Internal SearXNG API endpoint; SearXNG shares Gluetun's VPN network

### `SEARCH_SEARXNG_SERVER_BASE_URL`
- **Default**: `http://localhost:8080`
- **Description**: SearXNG's own base URL setting

### `SEARCH_SEARXNG_INSTANCE_NAME`
- **Default**: `Zone Search`
- **Description**: SearXNG instance display name

### `MODEL_SEARCH_PROXY_URL`
- **Default**: empty (direct catalog requests)
- **Description**: Optional HTTP proxy for remote model catalog searches from Manager
- **VPN value**: `http://gluetun:8888` in `.env` (Traefik). Manager's VPN overlay uses `http://127.0.0.1:8888` because Manager shares Gluetun's network namespace and cannot resolve the `gluetun` Docker DNS name.

### `TOOL_RUNNER_PROXY_URL`
- **Default**: empty (existing subprocess environment)
- **Description**: Optional HTTP proxy for proxy-aware command tools and MCP subprocesses
- **VPN value**: `http://gluetun:8888` in `.env` (Traefik). Manager's VPN overlay uses `http://127.0.0.1:8888` for the same shared-namespace reason.

### `ZONE_VPN`
- **Default**: empty
- **Description**: Set to `1` when the `vpn` Compose profile is active. Kept in
  sync with `COMPOSE_PROFILES` so internet-facing services attach to Gluetun's
  network namespace and all of their traffic uses the tunnel.
- **VPN value**: `1`

### `COMPOSE_PROFILES`
- **Default**: empty (core services only)
- **Description**: Comma-separated Compose profiles. Combine any of `dev`,
  `vpn`, `monitoring`, `bundled-ollama`, `bundled-comfyui`. Overlay files for
  `dev` and `vpn` are selected automatically.
- **Example**: `dev,vpn,monitoring`

`make up PROFILES=dev,vpn,monitoring` (or `./scripts/compose.sh --profile dev
--profile vpn --profile monitoring up`) saves `COMPOSE_PROFILES`, `COMPOSE_FILE`,
`ZONE_VPN`, and both proxy URLs in `.env` so rebuilds keep the same stack.
`make up` with no `PROFILES` starts core services only and clears them. The
VPN overlay is the network sandbox. Proxy URLs remain as belt-and-suspenders
for HTTP clients and Traefik ACME. A configured proxy does not silently fall
back to a direct connection when unavailable. The runner applies its proxy
settings after command and MCP environment overlays. Loopback and internal
service names bypass the proxy.

### Manager / zone-server chat

Compose and zone-server read the `SEARCH_*` names (not the older `RAG_*` aliases). When `SEARCH_ENABLE_WEB_SEARCH` is true, Manager chat automatically queries SearXNG when a message looks like it needs current web information (news, weather, prices, recency, URLs, etc.) and skips search for code review, casual replies, and stable knowledge questions. SearXNG shares Gluetun's network stack, so lookups leave through the VPN. When `ZONE_VPN=1`, Manager, LiteLLM, Grafana, and bundled engines share that stack too. Remote model catalog searches also use Gluetun's HTTP proxy when `MODEL_SEARCH_PROXY_URL` is configured. A message can force search on or off with `metadata.web_search`.

---

## 🧩 MCP servers (magents and others)

Zone's agent loop can attach [Model Context Protocol](https://modelcontextprotocol.io) servers as extra tools. That is how tasks and the CLI run [magents](https://github.com/abnegate/magents) — spawn or message Claude, Codex, Copilot, Cursor, Gemini, Grok, and OpenCode sessions from an agentic run.

Config uses the same JSON shape as Cursor (`mcpServers`). Tool names are prefixed with the server name, so magents' `spawn_session` becomes `magents_spawn_session`.

Stdio servers inherit the Zone process environment, then overlay any `env` map on the server spec. Treat configured servers as trusted local processes: they can see `PATH`, `HOME`, and whatever credentials the runner already has. Do not point Zone at an untrusted executable.

### `ZONE_MCP_ENABLED`
- **Default**: `true`
- **Description**: Master switch. `false` / `0` / `off` skips every MCP server.

### `ZONE_MCP_AUTO_MAGENTS`
- **Default**: `true`
- **Description**: When no servers are configured and `magents` is on `PATH`, attach `magents mcp` automatically.

### `ZONE_MCP_CONFIG`
- **Default**: *empty* (falls back to `~/.zone/mcp.json` if that file exists)
- **Description**: Path to a JSON file of MCP servers.
- **Example**:

```json
{
  "mcpServers": {
    "magents": {
      "command": "magents",
      "args": ["mcp"]
    }
  }
}
```

### `ZONE_MCP_SERVERS`
- **Default**: *empty*
- **Description**: Inline JSON of the same shape as `ZONE_MCP_CONFIG`. Useful in Compose. Takes precedence over the config file.

A server entry with only a `url` (HTTP transport) is skipped — Zone speaks stdio today.

Inside Docker the manager image does not include magents. Install it on the host and either run `zone-server` there, or mount the binary and a config file into the container.

---

## 🤖 Auto projects

An auto project runs itself: every agentic task in it is executed unattended, its pull request waits for checks, is reviewed by a model other than the one that wrote it and by the review bots already installed on the repository (CodeRabbit, Greptile), is fixed until nothing raised is left open, is merged — with administrator privileges when branch protection would otherwise refuse — and is reported with a high-level and a low-level summary. Start one from **Projects → Auto project**, which opens a planner chat that interviews you and creates the project and its tasks, or turn **Auto** on for an existing project. All settings are optional.

### `ZONE_AUTO_ENABLED`
- **Default**: `true`
- **Description**: Master switch for the driver. With it off, `POST /api/workspaces/{id}/projects/auto`, turning **Auto** on for a project and `POST /api/projects/{id}/automation/resume` answer `409`, since nothing on this server would pick the project up.

### `ZONE_AUTO_TICK_SECS`
- **Default**: `15` (5–300)
- **Description**: How often the driver looks for projects to advance. A finished run wakes it sooner.

### `ZONE_AUTO_PARALLEL_TASKS`
- **Default**: `3` (1–5)
- **Description**: Tasks one project may have in flight at once. Tasks whose dependencies are not merged wait.

### `ZONE_AUTO_MAX_ACTIVE_RUNS`
- **Default**: `4` (1–5)
- **Description**: Unattended runs across every project, so automation cannot take every execution slot from runs people start by hand.

### `ZONE_AUTO_MAX_RUNS_PER_TASK`
- **Default**: `3` (1–10)
- **Description**: Runs one task gets — the first, fix-ups after reviews, retries after failures — before the project pauses on it.

### `ZONE_AUTO_MAX_REVIEW_ROUNDS`
- **Default**: `4` (1–10)
- **Description**: Review rounds one pull request gets before the project pauses on it.

### `ZONE_AUTO_REVIEW_MODELS`
- **Default**: *empty*
- **Description**: Comma-separated models to review with, tried before the workspace's reasoning and fast models and the installed catalogue. The model that wrote a change never reviews it while another is available; successive rounds rotate reviewers.

### `ZONE_AUTO_REVIEW_REQUIRE_DISTINCT_MODEL`
- **Default**: `false`
- **Description**: When `true`, a change reviewed only by its own model — because nothing else is installed and no bot answered — pauses instead of merging. When `false`, the same model reviews under a reviewer persona and the merge notice says so.

### `ZONE_AUTO_REVIEW_BOTS`
- **Default**: *empty* (every bot this build knows: `coderabbit`, `greptile`)
- **Description**: Review bots to wait for and read. A bot named here is always expected; otherwise a bot is expected once it has commented on the repository.

### `ZONE_AUTO_BOT_REVIEW_GRACE_SECS`
- **Default**: `600` (60–3600)
- **Description**: How long to wait for an expected bot to review a head, measured from when the head's checks passed, before asking it with its trigger comment; then the same again from the moment it was asked, before going on without it.

### `ZONE_AUTO_CHECKS_GRACE_SECS`
- **Default**: `120` (30–1800)
- **Description**: How long checks may stay silent on a head before they count as absent. Absent checks are never skipped: the project's continuous-integration task is added or waited for and the branch is refreshed to run it.

### `ZONE_AUTO_CHECKS_TIMEOUT_SECS`
- **Default**: `3600` (300–86400)
- **Description**: How long checks may stay pending before the task pauses.

### `ZONE_AUTO_ADMIN_MERGE`
- **Default**: `true`
- **Description**: When branch protection refuses the merge, try again through the merge mutation an administrator merges with. Works when the project token belongs to a repository administrator the protection does not include, or a ruleset bypass actor.

### `ZONE_AUTO_DELETE_BRANCH`
- **Default**: `true`
- **Description**: Delete the branch once its pull request merged.

### `ZONE_AUTO_POST_MERGE_SECS`
- **Default**: `1800` (0–86400)
- **Description**: How long to watch the jobs a merge triggers on the base branch — deployments, releases. A failed job becomes a fix task; `0` watches nothing.

### `ZONE_AUTO_MAX_FIX_TASKS`
- **Default**: `5` (0–50)
- **Description**: Fix tasks the driver may add to one project for jobs that failed after a merge.

---

## 🔔 Notifications

Every notice — an auto-project merge, pause or completion, a regression alert, a scheduled digest — lands in the project's updates chat whatever is configured here. These add Slack, Discord and email on top.

### `ZONE_NOTIFY_SLACK_WEBHOOK`
- **Default**: *empty*
- **Description**: A Slack incoming-webhook URL. Only `hooks.slack.com` is accepted.

### `ZONE_NOTIFY_DISCORD_WEBHOOK`
- **Default**: *empty*
- **Description**: A Discord webhook URL. Only `discord.com` is accepted.

### `ZONE_NOTIFY_EMAIL_TO`
- **Default**: *empty*
- **Description**: Comma-separated recipients. Needs the `SMTP_*` relay below.

### `ZONE_NOTIFY_TIMEOUT_SECONDS`
- **Default**: `10` (max 120)
- **Description**: How long one channel is given per notice.

### `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_FROM_NAME`
- **Default**: *empty* (`SMTP_PORT` 587, `SMTP_FROM_NAME` Zone)
- **Description**: The relay notices, sign-up verification, password resets and invitations send through. `ALERT_SMTP_*` is Grafana's relay and is separate.

---

## 🔒 VPN Configuration - Optional

**VPN is completely optional!** Enable it to send all stack internet traffic through Gluetun, including private web search.

### `VPN_SERVICE_PROVIDER`
- **Default**: `surfshark`
- **Description**: VPN provider name
- **Options**: `surfshark`, `nordvpn`, `expressvpn`, `protonvpn`, `mullvad`, etc.
- **See**: https://github.com/qdm12/gluetun-wiki/tree/main/setup/providers

### `VPN_TYPE`
- **Default**: `openvpn`
- **Description**: VPN protocol to use
- **Options**: `openvpn`, `wireguard`
- **Note**: Check if your provider supports both

### `OPENVPN_USER`
- **Default**: *empty*
- **Description**: VPN account username
- **Surfshark**: Use your service credentials (not login email)
- **Required**: Only if using `--profile vpn`

### `OPENVPN_PASSWORD`
- **Default**: *empty*
- **Description**: VPN account password
- **Required**: Only if using `--profile vpn`

### `SERVER_COUNTRIES`
- **Default**: *commented out*
- **Description**: Pin VPN to specific countries (comma-separated)
- **Example**: `United States,Canada`
- **Usage**: Uncomment to use

### `SERVER_CITIES`
- **Default**: *commented out*
- **Description**: Pin VPN to specific cities (comma-separated)
- **Example**: `New York,Los Angeles`
- **Usage**: Uncomment to use

---

## 🐳 Docker Image Versions

### `DOCKER_VERSION_TRAEFIK`
- **Default**: `v3.7.12`
- **Description**: Traefik reverse proxy version
- **Note**: Paired with `DOCKER_DIGEST_TRAEFIK` for immutable resolution

### `DOCKER_VERSION_OLLAMA`
- **Default**: `0.33.2`
- **Description**: Ollama AI model runtime version
- **Note**: Used by both ollama and ollama-init services

### `DOCKER_VERSION_POSTGRES`
- **Default**: `pg16`
- **Description**: PostgreSQL database version for LiteLLM
- **Example**: `pg16`, `pg15`, `pg14`
- **Note**: Uses pgvector tags (e.g., `pg16`)

### `DOCKER_VERSION_LITELLM`
- **Default**: `v1.99.1`
- **Description**: LiteLLM proxy version
- **Note**: Paired with `DOCKER_DIGEST_LITELLM` for immutable resolution

### `DOCKER_VERSION_GLUETUN_BUNDLED`
- **Default**: `0.1.1-bundled`
- **Description**: Bundled Gluetun exporter image version
- **Note**: Only used when VPN profile is enabled

### `DOCKER_VERSION_SEARXNG`
- **Default**: `2026.9.3-a1144dda3`
- **Description**: SearXNG metasearch engine version
- **Note**: Only used when VPN profile is enabled

Every external image version has a matching `DOCKER_DIGEST_*` variable in
`.env.example`. Keep each tag and digest together when overriding an image.

---

## ⚙️ Advanced Configuration

### `LITELLM_WORKERS`
- **Default**: `4`
- **Description**: Number of LiteLLM worker processes
- **Range**: 1-8 recommended (1-2 per CPU core)
- **Higher**: Better concurrency, more RAM usage
- **Lower**: Less RAM, potential queuing

### `LITELLM_REQUEST_TIMEOUT`
- **Default**: `600` (10 minutes)
- **Description**: Maximum time for a single request (seconds)
- **Increase**: For very slow models or long responses
- **Decrease**: To fail fast on issues

### `LITELLM_ROUTER_TIMEOUT`
- **Default**: `120` (2 minutes)
- **Description**: Router decision timeout (seconds)
- **Usage**: How long to wait for routing decision

### `TZ`
- **Default**: `UTC`
- **Description**: Timezone for all containers
- **Example**: `America/New_York`, `Europe/London`, `Asia/Tokyo`
- **List**: https://en.wikipedia.org/wiki/List_of_tz_database_time_zones

### `ACME_EMAIL`
- **Default**: `admin@example.com`
- **Description**: Email for Let's Encrypt certificate notifications
- **Required**: For automatic HTTPS certificates
- **Usage**: Must be real email for certificate renewal notices

---

## 🎯 Configuration by Priority

### Tier 1: Zero Config (Default)
Just `cp .env.example .env` and it works!
- All variables have defaults
- Insecure keys for dev (warnings shown)

### Tier 2: Basic Security
Regenerate secrets for production:
- `LITELLM_MASTER_KEY` - `openssl rand -base64 32`
- `SEARXNG_SECRET_KEY` - `openssl rand -base64 32`

### Tier 3: Production
- `DOMAIN_HOST_WEBUI` - Your domain
- `ACME_EMAIL` - Your email
- `TZ` - Your timezone

### Tier 4: Optional Features
- VPN credentials - For private search
- Model changes - For performance tuning
- Docker versions - For version pinning
- Worker count - For scaling

---

## 📊 Summary Table

| Category | Required | Have Defaults |
|----------|----------|---------------|
| Domain | No | ✅ Yes |
| Security | No* | ✅ Yes (insecure) |
| Ollama Models | No | ✅ Yes |
| Model Backend | No | ✅ Yes |
| Web Search | No | ✅ Yes |
| VPN (optional) | No | ✅ Yes (empty OK) |
| Docker Versions | No | ✅ Yes |
| Advanced | No | ✅ Yes |
| MCP / magents | No | ✅ Yes (auto if `magents` is on PATH) |

*Security variables have insecure defaults. Change for production.

---

## 🚀 Instant Start Guide

### Absolute Minimum (3 commands, 30 seconds)
```bash
cp .env.example .env
mkdir -p auth && htpasswd -cB auth/users.htpasswd admin
make up
```

### Production Ready (1 command, 2 minutes)
```bash
./scripts/setup.sh
```

### With VPN Search (1 extra step)
```bash
nano .env  # Add OPENVPN_USER and OPENVPN_PASSWORD
make up-vpn
```

---

## 🔍 Variable Search Index

Need to find a specific config? Quick lookup:

- **Compose**: COMPOSE_PROFILES, ZONE_VPN
- **Authentication**: BASICAUTH_REALM, BASIC_AUTH_USERS_FILE
- **Docker Versions**: DOCKER_VERSION_TRAEFIK, DOCKER_VERSION_OLLAMA, DOCKER_VERSION_POSTGRES, DOCKER_VERSION_LITELLM, DOCKER_VERSION_GLUETUN, DOCKER_VERSION_SEARXNG, COMFYUI_COMMIT
- **Domains**: DOMAIN_HOST_WEBUI
- **Email**: ACME_EMAIL
- **Models**: OLLAMA_MODEL_FAST, OLLAMA_MODEL_REASON, OLLAMA_MODEL_EMBED
- **Model backend and coding agents**: ZONE_LLM_BACKEND,
  ZONE_LLM_BACKEND_EXECUTABLE, ZONE_AGENT_STATE_DIR, ZONE_AGENT_HOST_LOGIN,
  ZONE_CLAUDE_TOKEN_URL, ZONE_CODEX_SANDBOX, ZONE_AGENT_ENV_PASSTHROUGH,
  ZONE_CHAT_AGENT_CWD
- **Image, video, and audio generation**: COMFYUI_ENABLED, COMFYUI_BASE_URL,
  COMFYUI_WORKFLOW_PATH, COMFYUI_CHECKPOINT, COMFYUI_VIDEO_WORKFLOW_PATH,
  COMFYUI_VIDEO_UNET, COMFYUI_VIDEO_CLIP, COMFYUI_VIDEO_VAE,
  COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS, COMFYUI_AUDIO_WORKFLOW_PATH,
  COMFYUI_AUDIO_CHECKPOINT, COMFYUI_AUDIO_GENERATION_TIMEOUT_SECS,
  COMFYUI_COMMIT
- **Performance**: LITELLM_WORKERS, LITELLM_REQUEST_TIMEOUT, LITELLM_ROUTER_TIMEOUT
- **Search**: SEARCH_ENABLE_WEB_SEARCH, SEARCH_*, SEARXNG_*
- **MCP / magents**: ZONE_MCP_ENABLED, ZONE_MCP_AUTO_MAGENTS, ZONE_MCP_CONFIG, ZONE_MCP_SERVERS
- **Auto projects**: ZONE_AUTO_ENABLED, ZONE_AUTO_PARALLEL_TASKS, ZONE_AUTO_REVIEW_MODELS, ZONE_AUTO_REVIEW_BOTS, ZONE_AUTO_ADMIN_MERGE, ZONE_AUTO_*
- **Notifications**: ZONE_NOTIFY_SLACK_WEBHOOK, ZONE_NOTIFY_DISCORD_WEBHOOK, ZONE_NOTIFY_EMAIL_TO, SMTP_*
- **Security**: LITELLM_MASTER_KEY, LITELLM_SALT_KEY, SEARXNG_SECRET_KEY
- **Timezone**: TZ
- **VPN**: VPN_*, OPENVPN_*

---

**All configuration options are optional with working defaults**
