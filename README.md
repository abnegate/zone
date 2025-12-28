# Voiz - Self-Hosted AI Stack

A production-ready, privacy-focused AI stack featuring local LLM inference, semantic routing, and web search capabilities—all behind a secure VPN.

## Features

- **Local LLM Inference**: Run powerful language models locally with Ollama
- **Intelligent Routing**: Automatic model selection based on query complexity (LiteLLM)
- **ChatGPT-like Interface**: Modern web UI with conversation history (Open WebUI)
- **Private Web Search** (optional): VPN-protected metasearch engine (SearXNG + Gluetun)
- **Reverse Proxy**: Automatic HTTPS with Let's Encrypt (Traefik)
- **Security First**: Basic authentication, secrets management, no telemetry

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Internet                             │
└──────────────────────────┬──────────────────────────────────┘
                           │
                    ┌──────▼────────┐
                    │    Traefik    │  (Reverse Proxy + TLS)
                    │  Basic Auth   │
                    └───┬───────┬───┘
                        │       │
           ┌────────────┘       └────────────┐
           │                                 │
      ┌────▼─────┐                    ┌─────▼────┐
      │ Open     │───────────────────▶│ LiteLLM  │  (Semantic Routing)
      │ WebUI    │                    │  Proxy   │
      └────┬─────┘                    └─────┬────┘
           │                                 │
           │                          ┌──────▼──────┐
           │                          │   Ollama    │  (LLM Inference)
           │                          │             │  + GPU Support
           │                          └─────────────┘
           │
      ┌────▼──────────┐
      │   Gluetun     │  (VPN Tunnel)
      │               │
      │  ┌─────────┐  │
      │  │SearXNG  │  │  (Private Search)
      │  └─────────┘  │
      └───────────────┘
```

## Quick Start

**ZERO configuration required!** Just copy `.env.example` to `.env`, create basic auth, and start:

```bash
cp .env.example .env
mkdir -p auth && htpasswd -cB auth/users.htpasswd admin
make up
```

Access at `https://webui.localhost` (add to /etc/hosts if needed)

### Prerequisites

- **Docker** (20.10+) and **Docker Compose** (v2.0+)
- **8GB+ RAM** (16GB+ recommended for larger models)
- **50GB+ free disk space** (models can be large)
- **NVIDIA GPU** (optional, for faster inference)
- **VPN subscription** (optional, only needed for private web search)

### Installation

Choose your preferred installation method:

#### 🌐 Option 1: Web Installer (Recommended for First-Time Users)

Beautiful web-based wizard built with **Gleam** (type-safe functional language on the BEAM).

1. **Clone and start installer**

   ```bash
   git clone <repository-url>
   cd voiz
   make install
   ```

2. **Configure via web interface**

   - Open browser to `http://localhost:8000`
   - Step through 7-step configuration wizard
   - Generate secure secrets with one click (cryptographically secure)
   - Choose models based on your hardware
   - Optional: Configure VPN (OpenVPN or WireGuard)
   - Click "Install Now" and watch live progress

3. **Start the stack**

   ```bash
   make up          # Without VPN
   # or
   make up-vpn      # With VPN-protected search
   ```

**Tech Stack**: Gleam + Wisp + Mist (backend) | Vanilla HTML + JS + Tailwind (frontend)

#### ⚡ Option 2: Quick Start (Zero Configuration)

```bash
cp .env.example .env
mkdir -p auth && htpasswd -cB auth/users.htpasswd admin
make up
```

That's it! Uses insecure defaults (fine for development).

#### 💻 Option 3: CLI Setup Script

```bash
./scripts/setup.sh
```

Interactive command-line wizard for those who prefer the terminal.

---

### Post-Installation

After using any installation method above:

1. **Monitor initial setup** (models downloading)

   ```bash
   make logs-follow
   ```

   Wait for models to download (10-30 minutes depending on your connection).

6. **Access the web UI**

   Navigate to `https://your-webui-host.com` and log in with your basic auth credentials.

## Configuration

### Model Selection

Configure models in `.env` based on your hardware:

| Hardware | Fast Model | Reasoning Model | Embedding Model |
|----------|-----------|----------------|-----------------|
| 8GB RAM | `llama3.2:3b` | `deepseek-r1:7b` | `nomic-embed-text` |
| 16GB RAM | `llama3.1:8b` | `deepseek-r1:14b` | `nomic-embed-text` |
| 32GB RAM | `llama3.1:70b` | `deepseek-r1:32b` | `mxbai-embed-large` |

Browse more models at [Ollama Library](https://ollama.com/library).

### VPN Configuration (Optional)

**VPN is completely optional**. The system works perfectly without it - you'll just have direct (non-VPN) web search or no search at all.

To enable VPN-protected search:
1. Add VPN credentials to `.env`
2. Start with VPN profile: `docker compose --profile vpn up -d`

### VPN Providers

Gluetun supports many providers. See [Gluetun Wiki](https://github.com/qdm12/gluetun-wiki) for configuration:

- Surfshark (default)
- NordVPN
- ExpressVPN
- ProtonVPN
- Mullvad
- And many more...

### Resource Limits

Default resource limits are conservative. Adjust in `docker-compose.yml`:

```yaml
deploy:
  resources:
    limits:
      cpus: '4.0'
      memory: 4G
```

## Usage

### Makefile Commands

```bash
make help              # Show all available commands

# Setup
make setup             # Run interactive setup
make setup-auth        # Generate basic auth
make validate          # Validate configuration

# Operations
make up                # Start all services
make down              # Stop all services
make restart           # Restart all services
make logs              # Show recent logs
make logs-follow       # Follow logs
make ps                # Show service status

# Health & Monitoring
make health            # Check service health
make stats             # Show resource usage

# Model Management
make pull-models       # Manually pull models
make list-models       # List downloaded models

# Maintenance
make backup            # Backup volumes
make restore BACKUP=x  # Restore from backup
make clean             # Remove containers
make update            # Update images

# Development
make dev               # Start with live logs
make rebuild           # Rebuild and restart
make shell-ollama      # Shell into Ollama
```

### Manual Commands

```bash
# View logs for specific service
docker compose logs -f litellm

# Execute command in container
docker exec -it ollama ollama list

# Check VPN status
docker exec gluetun wget -qO- ifconfig.me

# Restart single service
docker compose restart openwebui
```

## API Usage

### LiteLLM OpenAI-Compatible API

```bash
curl https://api.yourdomain.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${LITELLM_MASTER_KEY}" \
  -d '{
    "model": "fast",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Model Selection

The system provides three models available in the Open WebUI interface:

- **auto** (default): Intelligently routes to fast or reason based on query complexity using semantic similarity
- **fast** (llama3.1:8b): Faster responses for general queries, summaries, rewrites, simple questions
- **reason** (deepseek-r1:14b): Slower but more thorough reasoning for complex analysis, debugging, system design

**How Auto Routing Works**:
1. Your query is analyzed using semantic embeddings (nomic-embed-text)
2. It's compared against example "utterances" for each model type
3. If the query is semantically similar to complex reasoning tasks, it routes to `reason`
4. If the query is semantically similar to simple tasks, it routes to `fast`
5. Default fallback is `fast` if no strong match is found

**Examples**:
- "Summarize this article" → routes to **fast**
- "Prove this algorithm is correct" → routes to **reason**
- "Analyze the time complexity of this code" → routes to **reason**
- "Translate this text" → routes to **fast**

**Manual Override**: You can always select `fast` or `reason` directly if you want to bypass auto routing.

**Automatic Fallback**: If any model fails, it automatically falls back to the reason model for reliability.

## Security

### Best Practices

1. **Never commit `.env` file** - Contains secrets!
2. **Use strong passwords** - For basic auth
3. **Rotate secrets regularly** - Master keys and VPN credentials
4. **Keep images updated** - Run `make update` monthly
5. **Review logs** - Monitor for suspicious activity
6. **Use VPN for search** - Already configured via Gluetun
7. **Enable fail2ban** - On the host system (optional)

### Secret Generation

Generate new secrets:

```bash
openssl rand -base64 32
```

Update basic auth password:

```bash
htpasswd -B auth/users.htpasswd username
```

## Troubleshooting

### Models not pulling

```bash
# Check ollama-init logs
docker compose logs ollama-init

# Manually pull a model
docker exec ollama ollama pull llama3.1:8b
```

### VPN not connecting

```bash
# Check Gluetun logs
docker compose logs gluetun

# Verify credentials in .env
# Check provider-specific requirements in Gluetun docs
```

### Out of memory

```bash
# Use smaller models
OLLAMA_MODEL_FAST=llama3.2:3b
OLLAMA_MODEL_REASON=deepseek-r1:7b

# Or reduce concurrent requests
LITELLM_WORKERS=2
```

### TLS certificate issues

```bash
# Check Traefik logs
docker compose logs traefik

# Verify ACME_EMAIL is set
# Ensure ports 80/443 are accessible
# Check DNS points to your server
```

### Service won't start

```bash
# Check service health
make health

# View specific service logs
docker compose logs <service-name>

# Restart service
docker compose restart <service-name>
```

## Backup & Recovery

### Backup

```bash
# Automatic backup to ./backups
make backup

# Manual backup of specific volume
docker run --rm \
  -v voiz_ollama_data:/data \
  -v $(pwd)/backups:/backup \
  alpine tar czf /backup/ollama.tar.gz -C /data .
```

### Restore

```bash
# Restore from backup
make restore BACKUP=backups/voiz_backup_20250101_120000.tar.gz

# Or manually
docker run --rm \
  -v voiz_ollama_data:/data \
  -v $(pwd)/backups:/backup \
  alpine tar xzf /backup/ollama.tar.gz -C /data
```

## Monitoring

### Resource Usage

```bash
make stats
```

### Health Checks

All services have health checks. View with:

```bash
make health
docker compose ps
```

### Logs

```bash
# All services
make logs-follow

# Specific service
docker compose logs -f ollama

# Tail last 100 lines
docker compose logs --tail=100
```

## Updating

### Update Docker Images

```bash
make update
```

### Update Models

```bash
# Remove old models
docker exec ollama ollama rm llama3.1:8b

# Pull new models
docker exec ollama ollama pull llama3.2:8b

# Or edit .env and restart
docker compose restart ollama-init
```

## Performance Tuning

### LiteLLM Workers

Increase for higher concurrency:

```env
LITELLM_WORKERS=8
```

### Ollama GPU Configuration

For multiple GPUs:

```yaml
deploy:
  resources:
    reservations:
      devices:
        - driver: nvidia
          device_ids: ['0', '1']
          capabilities: [gpu]
```

### Model Caching

Keep models in memory longer:

```env
OLLAMA_KEEP_ALIVE=24h
```

## Development

### Local Development

```bash
# Start with live logs
make dev

# Make changes and rebuild
make rebuild

# Open shell for debugging
make shell-ollama
```

### Adding Custom Models

Edit `.env`:

```env
OLLAMA_MODEL_FAST=your-custom-model:tag
```

Restart:

```bash
docker compose restart ollama-init
```

## System Requirements

### Minimum

- 4 CPU cores
- 8GB RAM
- 50GB disk space
- Docker 20.10+

### Recommended

- 8+ CPU cores
- 16GB+ RAM
- 100GB+ SSD
- NVIDIA GPU (6GB+ VRAM)
- Docker 24.0+

### Tested Platforms

- Ubuntu 22.04 LTS / 24.04 LTS
- Debian 11 / 12
- macOS (Docker Desktop)
- Windows 11 (Docker Desktop + WSL2)

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines.

## Acknowledgments

- [Ollama](https://ollama.com/) - Local LLM inference
- [LiteLLM](https://github.com/BerriAI/litellm) - LLM proxy and routing
- [Open WebUI](https://github.com/open-webui/open-webui) - Web interface
- [SearXNG](https://github.com/searxng/searxng) - Metasearch engine
- [Gluetun](https://github.com/qdm12/gluetun) - VPN client
- [Traefik](https://traefik.io/) - Reverse proxy

## Support

For issues, questions, or contributions:

1. Check [Troubleshooting](#troubleshooting) section
2. Review existing [GitHub Issues](issues)
3. Open a new issue with details:
   - Output of `make version`
   - Relevant logs from `make logs`
   - Steps to reproduce

---

**Built with privacy and performance in mind.**
