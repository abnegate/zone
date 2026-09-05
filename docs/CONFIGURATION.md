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
- **Default**: `nomic-embed-text`
- **Description**: Embedding model for semantic routing and search
- **RAM**: ~1-2GB
- **Critical**: Used by auto router to analyze query complexity
- **Options**:
  - `nomic-embed-text` (recommended, fast)
  - `mxbai-embed-large` (higher quality, slower)
- **Note**: If changed, restart needed to regenerate router.json

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

## 🎨 ComfyUI Image and Video Generation

See [COMFYUI.md](COMFYUI.md) for model setup, hardware requirements, checksum
details, and native macOS / bundled NVIDIA instructions.

### `COMFYUI_ENABLED`
- **Default**: `false`
- **Description**: Enables automatic image- and video-intent routing and direct
  ComfyUI generation
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
- **Description**: In-container path to the versioned FLUX.1 Schnell
  text-to-image API workflow. Image-to-image uses the sibling file
  `flux1-schnell-fp8-img2img-api.json` in the same directory.
- **Usage**: The Compose file mounts the repository workflow directory at this
  path

### `COMFYUI_CHECKPOINT`
- **Default**: `flux1-schnell-fp8.safetensors`
- **Description**: Fallback ComfyUI checkpoint when org/workspace AI settings
  do not set `model_image`. Path separators and traversal are rejected.
  Chat image generation uses the effective `model_image` setting when present.

### `COMFYUI_CLASSIFIER_MODEL`
- **Default**: `llama3.2:3b`
- **Description**: Fast LiteLLM model used when image-intent rules are unsure,
  including informal edits of an attached photo (`IMAGE` vs `CHAT`, 3-token
  reply). Org/workspace `model_fast` overrides this when set.
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
- **VPN value**: `http://gluetun:8888`

### `TOOL_RUNNER_PROXY_URL`
- **Default**: empty (existing subprocess environment)
- **Description**: Optional HTTP proxy for proxy-aware command tools and MCP subprocesses
- **VPN value**: `http://gluetun:8888`

`make up-vpn` and `make up-all` save both proxy URLs in `.env` so rebuilds retain
routing; `make up` clears both for a direct launch. A configured proxy does not
silently fall back to a direct connection when unavailable. The runner applies
its proxy settings after command and MCP environment overlays. Loopback and
internal service names bypass the proxy. Clients must honor proxy environment
variables; this is not a network sandbox for raw sockets or other clients that
ignore them. Tool HTTP requests go directly to Gluetun's proxy, while chat search
queries go to SearXNG.

### Manager / zone-server chat

Compose and zone-server read the `SEARCH_*` names (not the older `RAG_*` aliases). When `SEARCH_ENABLE_WEB_SEARCH` is true, Manager chat automatically queries SearXNG when a message looks like it needs current web information (news, weather, prices, recency, URLs, etc.) and skips search for code review, casual replies, and stable knowledge questions. SearXNG shares Gluetun's network stack, so lookups leave through the VPN. Remote model catalog searches use Gluetun's HTTP proxy when `MODEL_SEARCH_PROXY_URL` is configured. A message can force search on or off with `metadata.web_search`.

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

## 🔒 VPN Configuration - Optional

**VPN is completely optional!** Only needed if you want private web search.

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

- **Authentication**: BASICAUTH_REALM, BASIC_AUTH_USERS_FILE
- **Docker Versions**: DOCKER_VERSION_TRAEFIK, DOCKER_VERSION_OLLAMA, DOCKER_VERSION_POSTGRES, DOCKER_VERSION_LITELLM, DOCKER_VERSION_GLUETUN, DOCKER_VERSION_SEARXNG, COMFYUI_COMMIT
- **Domains**: DOMAIN_HOST_WEBUI
- **Email**: ACME_EMAIL
- **Models**: OLLAMA_MODEL_FAST, OLLAMA_MODEL_REASON, OLLAMA_MODEL_EMBED
- **Image and video generation**: COMFYUI_ENABLED, COMFYUI_BASE_URL,
  COMFYUI_WORKFLOW_PATH, COMFYUI_CHECKPOINT, COMFYUI_VIDEO_WORKFLOW_PATH,
  COMFYUI_VIDEO_UNET, COMFYUI_VIDEO_CLIP, COMFYUI_VIDEO_VAE,
  COMFYUI_VIDEO_GENERATION_TIMEOUT_SECS, COMFYUI_COMMIT
- **Performance**: LITELLM_WORKERS, LITELLM_REQUEST_TIMEOUT, LITELLM_ROUTER_TIMEOUT
- **Search**: SEARCH_ENABLE_WEB_SEARCH, SEARCH_*, SEARXNG_*
- **MCP / magents**: ZONE_MCP_ENABLED, ZONE_MCP_AUTO_MAGENTS, ZONE_MCP_CONFIG, ZONE_MCP_SERVERS
- **Security**: LITELLM_MASTER_KEY, LITELLM_SALT_KEY, SEARXNG_SECRET_KEY
- **Timezone**: TZ
- **VPN**: VPN_*, OPENVPN_*

---

**All configuration options are optional with working defaults**
