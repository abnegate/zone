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
- **Description**: Hostname for the Open WebUI interface
- **Example**: `ai.yourdomain.com`
- **Usage**: Configure DNS A record or add to /etc/hosts
- **Note**: LiteLLM API is internal-only, accessed via OpenWebUI

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
- **Usage**: Used by Open WebUI to authenticate to LiteLLM

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

## 💬 Open WebUI Configuration

### `WEBUI_AUTH`
- **Default**: `false`
- **Description**: Enable Open WebUI's built-in user authentication
- **Note**: We use Traefik basic auth instead by default
- **Set to `true`**: If you want per-user accounts in the UI

### `OPENAI_API_BASE_URL`
- **Default**: `http://litellm:4000/v1`
- **Description**: Internal URL for LiteLLM API endpoint
- **Usage**: Don't change unless customizing architecture

### `OPENAI_API_KEY`
- **Default**: *auto-set from LITELLM_MASTER_KEY*
- **Description**: API key for OpenAI-compatible requests
- **Usage**: Automatically configured by setup script

### `ENABLE_PERSISTENT_CONFIG`
- **Default**: `false`
- **Description**: Store config in database vs environment variables
- **Recommendation**: Keep `false` for infrastructure-as-code approach

### `ENABLE_SIGNUP`
- **Default**: `false`
- **Description**: Allow new users to create accounts
- **Set to `true`**: For multi-user installations

### `DEFAULT_LOCALE`
- **Default**: `en-US`
- **Description**: Default language for the interface
- **Options**: `en-US`, `es-ES`, `fr-FR`, `de-DE`, etc.

### `ENABLE_OLLAMA_API`
- **Default**: `false`
- **Description**: Enable direct Ollama API access in UI
- **Usage**: Keep `false` since we use LiteLLM proxy

### `ENABLE_OPENAI_API`
- **Default**: `true`
- **Description**: Enable OpenAI-compatible API in UI
- **Usage**: Must be `true` for LiteLLM integration

---

## 🔍 Web Search Configuration

### `ENABLE_RAG_WEB_SEARCH`
- **Default**: `true`
- **Description**: Enable web search in RAG (Retrieval Augmented Generation)
- **Note**: Only works when VPN profile is enabled

### `RAG_WEB_SEARCH_ENGINE`
- **Default**: `searxng`
- **Description**: Search engine to use for web search
- **Options**: `searxng` (only supported option)

### `RAG_WEB_SEARCH_RESULT_COUNT`
- **Default**: `5`
- **Description**: Number of search results to fetch per query
- **Range**: 1-20 (higher = more context, slower)

### `RAG_WEB_SEARCH_CONCURRENT_REQUESTS`
- **Default**: `8`
- **Description**: Maximum concurrent search requests
- **Range**: 1-16 (higher = faster parallel searches, more load)

### `SEARXNG_QUERY_URL`
- **Default**: `"http://gluetun:8080/search?q=<query>&format=json"`
- **Description**: SearXNG API endpoint URL
- **Note**: Quotes required for `<query>` placeholder
- **Usage**: Routes through Gluetun VPN container

### `SEARXNG_BASE_URL`
- **Default**: `"http://gluetun:8080"`
- **Description**: SearXNG base URL (through VPN)
- **Usage**: Internal routing through Gluetun

### `SEARXNG_SERVER_BASE_URL`
- **Default**: `http://localhost:8080`
- **Description**: SearXNG's own base URL configuration
- **Usage**: SearXNG internal setting

### `SEARXNG_INSTANCE_NAME`
- **Default**: `Zone Search`
- **Description**: Display name for SearXNG instance
- **Example**: `My Private Search`

### Manager / zone-server chat

Compose and zone-server read the `SEARCH_*` names (not the older `RAG_*` aliases). When `SEARCH_ENABLE_WEB_SEARCH` is true, Manager chat automatically queries SearXNG when a message looks like it needs current web information (news, weather, prices, recency, URLs, etc.) and skips search for code review, casual replies, and stable knowledge questions. SearXNG shares Gluetun's network stack, so lookups leave through the VPN. A message can force search on or off with `metadata.web_search`.

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
- **Default**: `v3.6`
- **Description**: Traefik reverse proxy version
- **Example**: `v3.6`, `v3.5`, `latest`
- **Note**: Pin to specific versions for production stability

### `DOCKER_VERSION_OLLAMA`
- **Default**: `0.13.5`
- **Description**: Ollama AI model runtime version
- **Example**: `0.13.5`, `0.13.0`, `latest`
- **Note**: Used by both ollama and ollama-init services

### `DOCKER_VERSION_POSTGRES`
- **Default**: `pg16`
- **Description**: PostgreSQL database version for LiteLLM
- **Example**: `pg16`, `pg15`, `pg14`
- **Note**: Uses pgvector tags (e.g., `pg16`)

### `DOCKER_VERSION_LITELLM`
- **Default**: `main-stable`
- **Description**: LiteLLM proxy version
- **Example**: `main-stable`, `main-latest`, specific commit SHA
- **Note**: `main-stable` recommended for production

### `DOCKER_VERSION_GLUETUN`
- **Default**: `v3.41`
- **Description**: Gluetun VPN client version
- **Example**: `v3.41`, `v3.40`, `latest`
- **Note**: Only used when VPN profile is enabled

### `DOCKER_VERSION_SEARXNG`
- **Default**: `latest`
- **Description**: SearXNG metasearch engine version
- **Example**: `latest`, specific tag
- **Note**: Only used when VPN profile is enabled

### `DOCKER_VERSION_OPENWEBUI`
- **Default**: `latest`
- **Description**: Open WebUI chat interface version
- **Example**: `latest`, `main`, specific tag
- **Note**: Update regularly for latest features

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
| Open WebUI | No | ✅ Yes |
| Web Search | No | ✅ Yes |
| VPN (optional) | No | ✅ Yes (empty OK) |
| Docker Versions | No | ✅ Yes |
| Advanced | No | ✅ Yes |

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

- **Authentication**: BASICAUTH_REALM, BASIC_AUTH_USERS_FILE, WEBUI_AUTH
- **Docker Versions**: DOCKER_VERSION_TRAEFIK, DOCKER_VERSION_OLLAMA, DOCKER_VERSION_POSTGRES, DOCKER_VERSION_LITELLM, DOCKER_VERSION_GLUETUN, DOCKER_VERSION_SEARXNG, DOCKER_VERSION_OPENWEBUI
- **Domains**: DOMAIN_HOST_WEBUI
- **Email**: ACME_EMAIL
- **Models**: OLLAMA_MODEL_FAST, OLLAMA_MODEL_REASON, OLLAMA_MODEL_EMBED
- **Performance**: LITELLM_WORKERS, LITELLM_REQUEST_TIMEOUT, LITELLM_ROUTER_TIMEOUT
- **Search**: SEARCH_ENABLE_WEB_SEARCH, SEARCH_*, SEARXNG_*
- **Security**: LITELLM_MASTER_KEY, LITELLM_SALT_KEY, SEARXNG_SECRET_KEY
- **Timezone**: TZ
- **VPN**: VPN_*, OPENVPN_*

---

**All configuration options are optional with working defaults**
