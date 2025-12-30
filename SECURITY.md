# Security Considerations for Voiz AI Stack

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
The `voiz_internal` network is designed to isolate:
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
The `voiz_edge` network connects:
- Traefik (reverse proxy)
- Open WebUI (user interface)
- LiteLLM (API gateway)
- Manager (admin interface)

Only Traefik should have ports exposed to the host.

## Authentication

### Basic Authentication
Traefik uses HTTP Basic Authentication for:
- Manager interface (`manager.${DOMAIN_HOST_WEBUI}`)
- Traefik dashboard (`traefik.${DOMAIN_HOST_WEBUI}`)

Credentials are stored in `./auth/users.htpasswd` using bcrypt hashing.

### WebUI Authentication
Open WebUI authentication is **ENABLED** by default (`WEBUI_AUTH=true`).

First user to register becomes admin. To restrict access:
```env
WEBUI_ENABLE_SIGNUP=false
WEBUI_DEFAULT_USER_ROLE=pending
```

### API Authentication
LiteLLM requires API key authentication via `SECURITY_LITELLM_MASTER_KEY`.

**Master Key Distribution**:
The master key is shared with:
1. **LiteLLM** - API gateway (generates the key)
2. **Open WebUI** - User interface (needs to call LiteLLM)
3. **Manager** - Admin interface (needs to register models in LiteLLM)

This distribution is minimized to only services that require it. The key is:
- Not exposed to the host
- Not exposed via environment variables to untrusted containers
- Transmitted only over internal Docker networks
- Never logged or stored in application logs

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
Custom containers (Manager, Installer) should run as non-root users. The Dockerfiles have been updated to include:
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
- [ ] Enable WebUI authentication
- [ ] Restrict signup or user registration
- [ ] Review prompt logging settings
- [ ] Update all Docker images
- [ ] Configure automated backups
- [ ] Set up monitoring and alerting
- [ ] Review Docker socket security
- [ ] Test authentication on all endpoints
- [ ] Validate secret key entropy
