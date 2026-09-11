# Contributing to Zone

Thank you for your interest in contributing to Zone! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Coding Standards](#coding-standards)
- [Project Structure](#project-structure)

## Code of Conduct

Be respectful, professional, and constructive in all interactions. We aim to maintain a welcoming environment for all contributors.

## Getting Started

1. **Fork the repository**

   Click the "Fork" button on GitHub to create your own copy.

2. **Clone your fork**

   ```bash
   git clone https://github.com/yourusername/zone.git
   cd zone
   ```

3. **Add upstream remote**

   ```bash
   git remote add upstream https://github.com/original/zone.git
   ```

4. **Create a feature branch**

   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites

- Docker 20.10+
- Docker Compose v2.0+
- Git
- Make (optional but recommended)
- Text editor or IDE

### Local Development Environment

1. **Configure for development**

   Edit `.env` with local development values:

   ```env
   DOMAIN_HOST_WEBUI=webui.localhost
   ADVANCED_LITELLM_WORKERS=2  # Lower for dev
   ```

2. **Generate test credentials**

   ```bash
   ./scripts/setup.sh
   ```

3. **Start development stack**

   ```bash
   make dev
   # or with VPN and Prometheus/Grafana:
   make dev PROFILES=vpn,monitoring
   ```

### Development Tips

- **Expose services locally**: Uncomment port mappings in `docker-compose.override.yml`
- **Disable VPN**: Use `make up` (not `make up-vpn`) for faster local testing
- **Combine profiles**: `make up PROFILES=dev,vpn,monitoring`
- **Use smaller models**: Set `OLLAMA_MODEL_FAST=llama3.2:3b` for faster startup
- **Enable debug logging**: Add `--debug` flag to LiteLLM command

## Making Changes

### Branch Naming

Use descriptive branch names:

- `feature/add-something` - New features
- `fix/bug-description` - Bug fixes
- `docs/update-readme` - Documentation updates
- `refactor/improve-component` - Code refactoring
- `test/add-tests` - Test additions

### Commit Messages

Write clear, descriptive commit messages:

```
type: Short summary (50 chars or less)

More detailed explanation if needed. Wrap at 72 characters.

- Bullet points are okay
- Reference issues: Fixes #123
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Examples:

```
feat: Add support for custom model configurations

fix: Resolve environment variable parsing in setup script

docs: Update README with VPN troubleshooting steps

refactor: Simplify LiteLLM routing configuration
```

## Testing

### Manual Testing

1. **Start fresh environment**

   ```bash
   make clean
   make up
   ```

2. **Verify all services start**

   ```bash
   make health
   ```

3. **Test core functionality**

   - Access web UI
   - Send test queries
   - Verify model routing
   - Test web search integration

4. **Check logs for errors**

   ```bash
   make logs
   ```

### Smoke Tests

Run basic automated tests:

```bash
make test
```

### Verifying a feature in the console

The Playwright suite under `manager/frontend/e2e` mocks every API call and signs
itself in with a forged token, so it can pass while nothing works. A feature is
not verified until it has been seen in the console against a real server.

`make live-verify` does that: it builds `zone-server`, migrates the database,
registers two tenants through the real registration endpoint, starts the console
against that server, and runs `manager/frontend/live` headless.

```bash
export DATABASE_URL=postgres://…   # the server migrates it on startup
export REDIS_URL=redis://127.0.0.1:6379
export JWT_SECRET=… ENCRYPTION_KEY=…

make live-verify                        # everything
make live-verify ARGS=live/train        # one file
ZONE_LIVE_KEEP=1 make live-verify       # leave the rig up to poke at

# Two checks drive a real agent end to end. They are skipped unless a model
# that reliably calls tools is named, because whether a model reaches for one
# is the model's decision. `llama3.2:3b` never did in five attempts.
ZONE_LIVE_AGENT_MODEL=qwen3.8:27b make live-verify
```

Chat, tasks and captions use the host Ollama. ComfyUI is replaced by a stand-in
that speaks the real `/prompt`, `/history`, `/view` protocol and returns real
PNG, WebM and FLAC bytes, so the server's whole media path runs without the
weights — each lane is asserted byte-exact against the fixture it should have
collected.

**The stand-in says nothing about what a model produces.** It is deliberately
silent on that, and a lane passing here is not evidence the model works: the
MPS upscale defect in `inference_hooks.py` passed every check in this suite. So
do not describe a stand-in run as verifying generation, upscaling, audio or
training.

### Verifying the models themselves

`live/real-media.live.ts` and `live/real-train.live.ts` drive the same console
against real ComfyUI and real weights, and assert the model's own output — that
an upscale is four times its source on both axes, that an audio clip is FLAC of
a real length, that a trained adapter beats its base by more than the 15% a
run that learned nothing scores. They need the weights and take tens of
minutes, so they skip unless asked for:

```bash
make setup-comfyui-macos                 # pinned native ComfyUI
PYTHON_BIN=python3.13 ./scripts/setup-comfyui-macos.sh --download-model --bundle upscale
make vision-model                        # U2-Net, for subject-aware crops

# Start that ComfyUI, then bring the rig up against it. ZONE_LIVE_REAL_MODELS=1
# skips the stand-in, points the server at the weights the installer put in
# place, leaves training to the server's own path, and sets deadlines a real
# render fits. Name the ComfyUI with ZONE_LIVE_COMFYUI_URL if it is not on
# 127.0.0.1:8188.
ZONE_LIVE_REAL_MODELS=1 ./scripts/live-verify.sh live/real-media.live.ts
ZONE_LIVE_REAL_MODELS=1 ZONE_TRAIN_CLIP=/path/to/subject.mp4 \
  ./scripts/live-verify.sh live/real-train.live.ts

# `make live-real` reruns the lanes against a rig that is already up, given a
# ZONE_LIVE_STATE naming the state file that rig wrote.
```

Name the real lanes explicitly rather than running the whole suite in this
mode: the stand-in lanes drive the stand-in's own control endpoints, which a
real ComfyUI does not serve.

Adapter quality also has its own measured check that needs no console:
`make test-lora-live` trains against the running ComfyUI and scores the result
against its base.

### Testing Checklist

Before submitting a PR, verify:

- [ ] All services start without errors
- [ ] Configuration validation passes (`make validate`)
- [ ] Web UI is accessible
- [ ] Basic chat functionality works
- [ ] Model pulling completes successfully
- [ ] No secrets committed to repository
- [ ] Documentation is updated
- [ ] Docker images use pinned versions

## Submitting Changes

### Pull Request Process

1. **Update from upstream**

   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Push to your fork**

   ```bash
   git push origin feature/your-feature-name
   ```

3. **Create Pull Request**

   - Go to GitHub and click "New Pull Request"
   - Select your feature branch
   - Fill out the PR template completely
   - Link related issues

### Pull Request Template

```markdown
## Description

Brief description of changes

## Type of Change

- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing

Describe how you tested your changes

## Checklist

- [ ] Code follows project style guidelines
- [ ] Documentation updated
- [ ] No secrets in commits
- [ ] All services start successfully
- [ ] Backward compatible (or breaking changes documented)
```

### Review Process

1. [CodeRabbit](https://coderabbit.ai) posts an AI review on the pull request
   (free for this public repository). On repositories with fewer than 10 stars,
   trigger it with `@coderabbitai review` or `@coderabbitai full review`.
2. GitHub Actions CI runs lint, tests, and other checks
3. A maintainer reviews the change
4. Address review feedback
5. Approval and merge

Common CodeRabbit commands:

- `@coderabbitai review` — incremental review of new commits
- `@coderabbitai full review` — review the entire pull request again
- `@coderabbitai summary` — regenerate the PR summary
- `@coderabbitai ignore` — skip further reviews on this PR

## Coding Standards

### Shell Scripts

- Use `#!/usr/bin/env bash` or `#!/bin/sh`
- Include `set -euo pipefail` for bash scripts
- Use `readonly` for constants
- Quote all variables: `"${VARIABLE}"`
- Add error handling
- Include usage comments

Example:

```bash
#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log_info() {
    echo "[INFO] $1"
}

main() {
    log_info "Starting process..."
    # Implementation
}

main "$@"
```

### Docker Compose

- Pin all image versions (no `:latest`)
- Use health checks for all services
- Add resource limits
- Use environment variables for configuration
- Include clear comments
- Group related services

### YAML

- Use 2-space indentation
- Keep lines under 100 characters
- Use consistent quoting style
- Add inline comments for complex configurations

### Documentation

- Use GitHub-flavored Markdown
- Include code examples
- Add table of contents for long docs
- Keep language clear and concise
- Update README when adding features

## Project Structure

```
zone/
├── .env.example              # Environment configuration template
├── .gitignore                # Git ignore rules
├── docker-compose.yml        # Main service definitions
├── docker-compose.dev.yml    # Hot-reload overlay (`dev` profile)
├── docker-compose.vpn.yml    # Full-tunnel overlay (`vpn` profile)
├── Makefile                  # Common tasks
├── README.md                 # Main documentation
├── CONTRIBUTING.md           # This file
│
├── auth/                     # Basic authentication
│   └── users.htpasswd        # Generated by setup
│
├── litellm/                  # LiteLLM configuration
│   └── config.yaml           # Routing and model config
│
├── ollama/                   # Ollama configuration
│   └── pull-models.sh        # Model initialization
│
├── searxng/                  # SearXNG configuration
│   └── settings.yml          # Search engine settings
│
├── scripts/                  # Utility scripts
│   └── setup.sh              # Interactive setup
│
└── backups/                  # Backup directory (gitignored)
```

### Adding New Services

1. Add service to `docker-compose.yml`
2. Pin image version
3. Add health check
4. Add resource limits
5. Configure networks appropriately
6. Update `.env.example` with new variables
7. Document in README.md
8. Add Makefile targets if needed

### Modifying Configurations

1. Update relevant config file
2. Update `.env.example` if adding variables
3. Update `docker-compose.yml` if changing service config
4. Test changes locally
5. Update documentation
6. Note breaking changes in PR description

## Security Considerations

### Never Commit

- Real `.env` files
- `auth/users.htpasswd`
- Private keys or certificates
- VPN credentials
- API keys

### Always

- Use `.env.example` for templates
- Pin Docker image versions
- Validate user input in scripts
- Use secure defaults
- Document security implications

## Documentation Guidelines

### README Updates

When adding features, update:

- Features list
- Configuration section
- Usage examples
- Troubleshooting if applicable
- Architecture diagram if needed

### Code Comments

- Explain **why**, not **what**
- Document complex logic
- Add TODO comments for future improvements
- Use consistent comment style

### Configuration Comments

- Explain purpose of each setting
- Document valid values/ranges
- Link to external documentation
- Warn about security implications

## Getting Help

- **Issues**: Search existing issues or open a new one
- **Discussions**: Use GitHub Discussions for questions
- **Documentation**: Check README and wiki
- **Examples**: Look at `docker-compose.override.yml.example`

## Recognition

Contributors will be acknowledged in:

- Release notes
- README contributors section
- Git commit history

Thank you for contributing to Zone!
