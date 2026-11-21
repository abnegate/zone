# Security Considerations for Zone AI Stack

## Host folder mounts

Compose bind-mounts `ZONE_HOST_ROOT` at `/host` in the manager. Workspace
**Folders** (and the first-run setup step) store host paths under that root;
chat tools then open the matching `/host/...` path as the `zone` user. Mounting
`$HOME` makes every file under home reachable from any chat in any workspace.
Pick the smallest parent that covers the folders you will select. Recreate the
manager after changing `ZONE_HOST_ROOT`. An empty value mounts
`./.zone/host-empty`, so nothing on the host is visible until a root is set.

## Docker Socket Access (Traefik)

### Risk Assessment
Traefik requires access to the Docker socket (`/var/run/docker.sock`) to:
- Automatically discover containers and configure routes
- Read container labels for routing configuration
- Monitor container lifecycle events

**Security Implications**: Access to the Docker socket is equivalent to root access on the host system. A compromised Traefik container could:
- Start/stop any container
- Read secrets from any container
- Escalate privileges
- Access host filesystem

### Mitigation Options

#### Option 1: Docker Socket Proxy (Recommended for Production)
Use a socket proxy like [tecnativa/docker-socket-proxy](https://github.com/Tecnativa/docker-socket-proxy) to limit Docker API access:

```yaml
services:
  docker-proxy:
    image: tecnativa/docker-socket-proxy:latest
    container_name: docker-proxy
    restart: unless-stopped
    environment:
      - CONTAINERS=1  # Allow listing containers
      - NETWORKS=1    # Allow listing networks
      - SERVICES=0    # Deny services access
      - TASKS=0       # Deny tasks access
      - POST=0        # Deny POST (create/update)
      - DELETE=0      # Deny DELETE
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
    networks:
      - internal

  traefik:
    # ... existing config ...
    volumes:
      # Replace socket mount with proxy connection
      - docker-proxy:2375  # Instead of /var/run/docker.sock
    environment:
      - DOCKER_HOST=tcp://docker-proxy:2375
```

#### Option 2: Accept the Risk (Current Configuration)
The current configuration mounts the Docker socket as read-only (`:ro`). This is acceptable for:
- Development environments
- Self-hosted setups where the host is already trusted
- Environments where Traefik is the only entry point

**Justification for Current Approach**:
1. Traefik is a trusted, widely-used reverse proxy
2. The socket is mounted read-only
3. Traefik runs on an isolated Docker network
4. The project is designed for self-hosted, single-user deployments

### Best Practices
1. Keep Traefik updated to patch security vulnerabilities
2. Review Traefik configuration regularly
3. Monitor Traefik logs for suspicious activity
4. Consider using a socket proxy in production deployments
5. Restrict network access to the Traefik container

### Additional Resources
- [Docker Socket Security](https://docs.docker.com/engine/security/#docker-daemon-attack-surface)
- [Traefik Security Documentation](https://doc.traefik.io/traefik/providers/docker/#security-considerations)
- [Docker Socket Proxy Project](https://github.com/Tecnativa/docker-socket-proxy)

## Database Credentials

### Default Password Risk
The default configuration includes fallback passwords for development:
- PostgreSQL: `litellm_password`
- LiteLLM Master Key: `sk-dev-insecure-key-change-for-production`
- SearXNG Secret: `dev-insecure-key-change-for-production`

**CRITICAL**: These MUST be changed for any production or internet-accessible deployment.

### Secure Setup
Use the provided setup script to generate secure credentials:
```bash
./scripts/setup.sh
```

Or manually generate secure keys:
```bash
# Generate 32-byte base64 keys
openssl rand -base64 32
```

## Network Isolation

### Internal Network
The `zone_internal` network is designed to isolate:
- Ollama (LLM backend)
- PostgreSQL database
- LiteLLM proxy
- SearXNG search engine

**Configuration**: Set `internal: true` to prevent external access:
```yaml
networks:
  internal:
    internal: true  # Blocks all external traffic
```

**Note**: The current configuration has `internal: false` to support certain deployment scenarios. Review your threat model and adjust accordingly.

### Edge Network
The `zone_edge` network connects:
- Traefik (reverse proxy)
- Zone console (chat and workspace interface)
- LiteLLM (API gateway)
- Manager API

Only Traefik should have ports exposed to the host.

## Authentication

### Basic Authentication
Traefik uses HTTP Basic Authentication for:
- Manager interface (`manager.${DOMAIN_HOST_WEBUI}`)
- Traefik dashboard (`traefik.${DOMAIN_HOST_WEBUI}`)

Credentials are stored in `./auth/users.htpasswd` using bcrypt hashing.

### Zone Chat Authentication

Zone chat uses Manager's account authentication and workspace access controls.
Chat history and agent actions belong to the authenticated user's workspace.

### API Authentication
LiteLLM requires API key authentication via `SECURITY_LITELLM_MASTER_KEY`.

**Master Key Distribution**:
The master key is shared with:
1. **LiteLLM** - API gateway (generates the key)
2. **Manager** - Backend model registration and inference requests

This distribution is minimized to only services that require it. The key is:
- Not exposed to the host
- Not exposed via environment variables to untrusted containers
- Transmitted only over internal Docker networks
- Never logged or stored in application logs

## Tool HTTP Routing

`TOOL_RUNNER_PROXY_URL` sets proxy environment variables for command tools and
MCP subprocesses after their normal environment overlays. VPN launches configure
Gluetun's HTTP proxy. Proxy-aware clients fail if that configured proxy is
unavailable; loopback and internal service destinations bypass it. This covers
clients that honor the proxy environment, not raw sockets or clients that ignore
it. SearXNG handles chat search queries; generic tool HTTP requests use Gluetun's
proxy directly.

HTTP MCP servers are not subprocesses: Zone connects to them from its own
process. Those connections go through the same proxy, reach loopback and
internal service destinations directly, and never use `HTTP_PROXY`,
`HTTPS_PROXY` or the system proxy. Private and loopback hosts stay reachable on
purpose, so an organization admin can point a server at any host the Zone
server can reach. Link-local and cloud metadata addresses are refused, both as
written in a server's URL and as any address its host name resolves to; behind
the proxy, which resolves names itself, only an address or metadata host written
in the URL is refused. Only a same-origin 307 or 308 redirect is followed. A
server that fails shows a sentence chosen by two typed values the MCP client
records: the kind of failure, and the status of the HTTP answer that failed it.
No text of the answer or of the error is read to choose the sentence or copied
into it, so a server pointed at an internal host does not read that host's
answer back to a tenant.

A chat's Offline switch keeps the chat's agent off every host beyond the
chat's own model provider and Zone's own model infrastructure (embeddings, the
title and classifier models, Ollama, ComfyUI, LiteLLM). An offline chat is
offered only the tools on one allow-list (`agent::offline::TOOLS`): Zone's file
and shell tools, its own workspace, memory and document tools, and the image,
audio and model tools that go through that infrastructure. Every other tool,
including any added later, is withheld: web search and page fetch, MCP servers,
GitHub tools, starting task runs, monitoring and model installs among them. The
tools on the list that could reach further refuse those calls: remote model
catalogues, remote training compute, and waits on pull request checks. An
offline chat clones no project, and Zone never reads or changes its pull
requests on GitHub, so the watcher and auto-fix leave them alone.
`run_command` refuses `curl` and `wget`; any other network access from a shell
command, such as a script, a package manager or a raw socket, is not closed by
the tool gates and relies on the operating-system network deny Zone places on
an offline chat's child processes.

That deny needs seccomp and Landlock on Linux, or the sandbox library on macOS.
A host without them cannot start a network-denied command, so an offline chat's
commands fail closed there instead of running with a network. Zone starts a
trivial denied command at startup and logs a warning when it does not run, and
the new-chat dialog's Offline option says that offline chats on that host
cannot run commands (`offline_enforced` in
`GET /api/workspaces/{id}/chat-providers`).

A coding agent such as Claude Code or Codex is never left its own built-in
tools in an offline chat, even when the chat turns Zone tools only off: those
tools reach the network without Zone seeing them, so offline overrides
unsandboxed and the agent gets Zone's tools alone.

An agent policy's `domains` rule (see [docs/POLICY.md](docs/POLICY.md#network))
builds on the same gates. While either the organization's or the workspace's
policy sets it, every chat and task run in the workspace is held to everything
above, as if its Offline switch were on, and Zone then lets back in only
`fetch_url` to the hosts the policy allows, checked before the request and
after every redirect, and HTTP MCP servers whose host it allows. Web search,
GitHub tools, stdio MCP servers and the agents' own built-in tools stay
withheld, and shell commands keep the operating-system network deny. A chat
that is offline in its own right gets nothing back. Zone's own traffic is not
agent egress and does not follow the rule: the chat's model provider, Zone's
model infrastructure, its project clones and pull request calls to GitHub,
and knowledge source sync.

A policy's `providers` rule judges a LiteLLM endpoint by the routes it
publishes at `/v2/model/info`. That answer is the endpoint's own, so Zone
believes it only from the instance's `LITELLM_HOST` or an endpoint saved in the
organization's AI settings. An endpoint saved in a workspace's AI settings
counts as an unidentified provider, which a restricted policy denies, so a
workspace admin cannot satisfy the organization's rule with a proxy that
misreports its routes (see
[docs/POLICY.md](docs/POLICY.md#how-zone-tells-where-a-litellm-model-runs)).

## Audit Destinations

Audit destinations ([docs/AUDIT.md](docs/AUDIT.md)) send an organization's or
the server's audit log to a webhook, an S3-compatible store or a syslog
server. Their secrets, a webhook signing secret, S3 access keys and syslog
certificates and keys, are sealed in `audit_destinations.secrets` under the
master key derived from `ENCRYPTION_KEY`, in the same `ENC[v1:…]` envelopes as
other credentials, and `zone-server --seal-secrets` covers that column (see
[docs/DEPLOY.md](docs/DEPLOY.md#sealing-stored-credentials)). They are never
returned by the API, which lists only the names of the secrets set, never
logged and never recorded in the audit log. Secrets that don't open under the
key fail delivery with `unsealable` rather than being sent without them.

Private, loopback and LAN hosts are allowed on purpose, over `http` or
`https`, so a self-hosted SIEM or object store can receive the log. Link-local
and cloud metadata addresses are refused when a destination is saved, again
before every delivery, and among the addresses a host name resolves to when
sending. `ZONE_ENDPOINT_HOSTS` limits destinations to the hosts it lists and
is re-checked before every delivery, so narrowing it stops existing
destinations too. Delivery uses no proxy and follows no redirect. A failed
delivery records a failure code and a sentence of Zone's own; the HTTP status
code is the only part of an answer it keeps, never a response body or header,
so a destination pointed at an internal host doesn't read that host's answer
back to an organization admin.

Plain `http` and UDP syslog send events unencrypted, and the console warns
when a destination uses them.

Every audit log download writes an `audit_export.downloaded` row before the
first byte is sent, and is refused with `500` when that row can't be written,
so no export goes unrecorded. Downloads are sent with `Cache-Control:
no-store`.

## Privacy Considerations

### Prompt Logging
LiteLLM is configured to **NOT** store prompts in spend logs by default:
```yaml
general_settings:
  store_prompts_in_spend_logs: false  # Privacy-first configuration
```

### Search Privacy
SearXNG routes search traffic through VPN (when enabled) to:
- Mask IP addresses from search providers
- Prevent search tracking
- Enable access to region-restricted content

## TLS/HTTPS

### Let's Encrypt (Production)
Enable automatic TLS certificates:
```env
SECURITY_GENERATE_CERTIFICATE=true
SECURITY_HTTP_REDIRECT=true
ADVANCED_ACME_EMAIL=your-email@example.com
```

**Requirements**:
- Valid public domain
- Ports 80/443 accessible from internet
- Domain points to your server

### Self-Signed Certificates (Development)
For local development, use self-signed certificates or accept HTTP connections.

## Container Security

### Non-Root Users
Custom containers (Manager) should run as non-root users. The Dockerfiles have been updated to include:
```dockerfile
USER nonroot:nonroot
```

### Image Updates
Regularly update Docker images to patch security vulnerabilities:
```bash
docker compose pull
docker compose up -d
```

## Reporting Security Issues

If you discover a security vulnerability, please report it responsibly:
1. Do NOT open a public GitHub issue
2. Email the project maintainer with details
3. Allow time for a patch before public disclosure

## Security Checklist

Before deploying to production:
- [ ] Change all default passwords and secrets
- [ ] Enable TLS with valid certificates
- [ ] Review network isolation settings
- [ ] Disable unnecessary services
- [ ] Configure firewall rules
- [ ] Verify Manager authentication and workspace access
- [ ] Restrict signup or user registration
- [ ] Review prompt logging settings
- [ ] Update all Docker images
- [ ] Configure automated backups
- [ ] Set up monitoring and alerting
- [ ] Review Docker socket security
- [ ] Test authentication on all endpoints
- [ ] Validate secret key entropy
