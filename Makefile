.PHONY: help setup up down restart logs logs-follow ps health check \
	pull-models clean clean-volumes backup restore \
	setup-auth add-user setup-comfyui-macos setup-comfyui-model \
	verify-comfyui-model validate test \
	up-vpn up-monitoring up-comfyui up-all dev rebuild update \
	shell-ollama shell-litellm shell-openwebui shell-manager shell-console \
	shell-postgres shell-valkey db-shell db-migrate \
	test-server test-console test-console-coverage test-e2e test-e2e-ui \
	lint-console format-console check-console \
	list-models stats prune version env urls install \
	sqlx-prepare \
	build-runner test-runner setup-runner-coverage test-runner-coverage \
	test-runner-coverage-html test-runner-coverage-json test-runner-coverage-text \
	install-runner install-cli \
	build-dev-cli install-dev-cli dev-format dev-format-check dev-lint dev-test dev-coverage dev-check \
	kind-create kind-delete tilt-up tilt-down kind-status \

.DEFAULT_GOAL := help

# Colors for output
BLUE := \033[0;34m
GREEN := \033[0;32m
YELLOW := \033[1;33m
RED := \033[0;31m
NC := \033[0m

# Docker compose command (try both v1 and v2)
DOCKER_COMPOSE := $(shell which docker-compose 2>/dev/null || echo "docker compose")

##@ Setup & Configuration

install: ## Start web-based installer (recommended for first-time setup)
	@echo "$(BLUE)Starting web installer...$(NC)"
	@echo "$(GREEN)Building installer container...$(NC)"
	@$(DOCKER_COMPOSE) --profile installer build installer
	@$(DOCKER_COMPOSE) --profile installer up installer
	@echo ""
	@echo "$(GREEN)Web installer started!$(NC)"
	@echo "$(BLUE)Open your browser to: http://localhost:8000$(NC)"
	@echo ""
	@echo "Press Ctrl+C when done to stop the installer"

setup: ## Run interactive CLI setup script
	@echo "$(BLUE)Running setup script...$(NC)"
	@./scripts/setup.sh

setup-comfyui-macos: ## Install pinned native ComfyUI on Apple Silicon (model excluded)
	@./scripts/setup-comfyui-macos.sh

setup-comfyui-model: ## Explicitly download and checksum-verify FLUX.1 Schnell FP8 (~17.2 GB)
	@$(DOCKER_COMPOSE) --profile comfyui-model-setup run --rm comfyui-model-setup \
		python /opt/zone/download-models.py \
		--manifest /opt/zone/model-manifest.json \
		--models-dir /models \
		$(if $(filter 1 true yes,$(FORCE)),--force,)

verify-comfyui-model: ## Verify the installed FLUX.1 Schnell FP8 size and SHA-256
	@$(DOCKER_COMPOSE) --profile comfyui-model-setup run --rm comfyui-model-setup \
		python /opt/zone/download-models.py \
		--manifest /opt/zone/model-manifest.json \
		--models-dir /models \
		--verify-only

setup-auth: ## Generate basic auth credentials
	@echo "$(BLUE)Setting up basic authentication...$(NC)"
	@mkdir -p auth
	@read -p "Enter username: " username; \
	htpasswd -cB auth/users.htpasswd $$username

add-user: ## Add additional basic auth user
	@echo "$(BLUE)Adding user to basic authentication...$(NC)"
	@read -p "Enter username: " username; \
	htpasswd -B auth/users.htpasswd $$username

validate: ## Validate configuration
	@echo "$(BLUE)Validating configuration...$(NC)"
	@if [ -f .env ] && [ -f auth/users.htpasswd ]; then \
		echo "$(GREEN)✓ .env file exists$(NC)"; \
		echo "$(GREEN)✓ auth file exists$(NC)"; \
		$(DOCKER_COMPOSE) config --quiet && echo "$(GREEN)✓ Docker Compose config is valid$(NC)" || echo "$(RED)✗ Docker Compose config is invalid$(NC)"; \
	else \
		echo "$(RED)✗ Missing .env or auth/users.htpasswd. Run 'make setup' first.$(NC)"; \
		exit 1; \
	fi

##@ Docker Operations

up: ## Start all services (without VPN or monitoring)
	@echo "$(GREEN)Starting services...$(NC)"
	$(DOCKER_COMPOSE) up -d
	@echo "$(GREEN)Services started! Check status with: make ps$(NC)"
	@echo "$(YELLOW)Note: VPN not enabled. For VPN-protected search, use: make up-vpn$(NC)"

up-vpn: ## Start all services with VPN-protected search
	@echo "$(GREEN)Starting services with VPN...$(NC)"
	$(DOCKER_COMPOSE) --profile vpn up -d
	@echo "$(GREEN)Services started with VPN! Check status with: make ps$(NC)"

up-monitoring: ## Start all services with monitoring (Prometheus + Grafana)
	@echo "$(GREEN)Starting services with monitoring...$(NC)"
	$(DOCKER_COMPOSE) --profile monitoring up -d
	@echo "$(GREEN)Services started with monitoring! Check status with: make ps$(NC)"
	@echo "$(BLUE)Grafana: http://grafana.$${DOMAIN_HOST_WEBUI:-localhost}$(NC)"
	@echo "$(BLUE)Prometheus: http://prometheus.$${DOMAIN_HOST_WEBUI:-localhost}$(NC)"

up-comfyui: verify-comfyui-model ## Start the bundled NVIDIA ComfyUI runtime
	@echo "$(GREEN)Starting bundled NVIDIA ComfyUI...$(NC)"
	$(DOCKER_COMPOSE) --profile bundled-comfyui up -d comfyui

up-all: ## Start all services with VPN and monitoring
	@echo "$(GREEN)Starting all services (VPN + monitoring)...$(NC)"
	$(DOCKER_COMPOSE) --profile vpn --profile monitoring up -d
	@echo "$(GREEN)All services started! Check status with: make ps$(NC)"

down: ## Stop all services
	@echo "$(YELLOW)Stopping services...$(NC)"
	$(DOCKER_COMPOSE) --profile vpn --profile monitoring --profile installer \
		--profile bundled-comfyui --profile comfyui-model-setup down

restart: ## Restart all services
	@echo "$(YELLOW)Restarting services...$(NC)"
	$(DOCKER_COMPOSE) restart

ps: ## Show service status
	@$(DOCKER_COMPOSE) --profile vpn --profile monitoring ps

logs: ## Show recent logs (non-following)
	@$(DOCKER_COMPOSE) logs --tail=100

logs-follow: ## Follow logs from all services
	@$(DOCKER_COMPOSE) logs -f

logs-service: ## Follow logs for a specific service (usage: make logs-service SERVICE=ollama)
	@$(DOCKER_COMPOSE) logs -f $(SERVICE)

##@ Kind + Tilt (Local Kubernetes)

kind-create: ## Create Kind cluster with CNPG and HAProxy
	@./scripts/kind-create.sh

kind-delete: ## Delete Kind cluster
	@./scripts/kind-delete.sh

kind-status: ## Show Kind cluster status
	@echo "$(BLUE)Kind Cluster Status:$(NC)"
	@kind get clusters 2>/dev/null | grep -q "zone-dev" && echo "$(GREEN)Cluster 'zone-dev' is running$(NC)" || echo "$(YELLOW)Cluster 'zone-dev' not found$(NC)"
	@echo ""
	@if kind get clusters 2>/dev/null | grep -q "zone-dev"; then \
		echo "$(BLUE)Pods in zone namespace:$(NC)"; \
		kubectl get pods -n zone 2>/dev/null || echo "No pods found"; \
	fi

tilt-up: ## Start Tilt development environment
	@./scripts/tilt-up.sh

tilt-down: ## Stop Tilt and clean up
	@echo "$(YELLOW)Stopping Tilt...$(NC)"
	@cd k8s && tilt down
	@echo "$(GREEN)Tilt stopped. Kind cluster still running.$(NC)"
	@echo "$(YELLOW)Run 'make kind-delete' to remove the cluster entirely.$(NC)"

##@ Health & Monitoring

health: ## Check health status of all services
	@echo "$(BLUE)Service Health Status:$(NC)"
	@$(DOCKER_COMPOSE) --profile vpn --profile monitoring ps --format json | jq -r '.[] | "\(.Name): \(.Health)"' 2>/dev/null || \
	$(DOCKER_COMPOSE) --profile vpn --profile monitoring ps | grep -E '(Up|Exited|Restarting)'

check: health ## Alias for health check

stats: ## Show resource usage
	@docker stats --no-stream --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}"

##@ Model Management

pull-models: ## Pull models into the host Ollama daemon
	@echo "$(BLUE)Pulling Ollama models...$(NC)"
	@if ! command -v ollama >/dev/null 2>&1; then \
		echo "$(RED)ollama CLI not found. Install from https://ollama.com$(NC)"; \
		echo "$(YELLOW)Or use the bundled engine: docker compose --profile bundled-ollama run --rm ollama-init$(NC)"; \
		exit 1; \
	fi
	@if ! curl -sf http://127.0.0.1:11434/api/tags >/dev/null; then \
		echo "$(RED)Host Ollama is not reachable at http://127.0.0.1:11434$(NC)"; \
		echo "$(YELLOW)Start it with: ollama serve$(NC)"; \
		echo "$(YELLOW)Or use the bundled engine: docker compose --profile bundled-ollama run --rm ollama-init$(NC)"; \
		exit 1; \
	fi
	@set -a; [ -f .env ] && . ./.env; set +a; \
	ollama pull $${OLLAMA_MODEL_FAST:-llama3.1:8b}; \
	ollama pull $${OLLAMA_MODEL_REASON:-deepseek-r1:32b}; \
	ollama pull $${OLLAMA_MODEL_EMBED:-nomic-embed-text}; \
	echo "$(GREEN)Models pulled.$(NC)"

list-models: ## List downloaded Ollama models
	@echo "$(BLUE)Downloaded Ollama models:$(NC)"
	@if curl -sf http://127.0.0.1:11434/api/tags >/dev/null; then \
		ollama list; \
	elif docker ps --format '{{.Names}}' | grep -q '^ollama$$'; then \
		docker exec ollama ollama list; \
	else \
		echo "$(RED)Ollama is not running. Start the host daemon or use --profile bundled-ollama.$(NC)"; \
		exit 1; \
	fi

##@ Database Operations

db-shell: ## Open PostgreSQL shell
	@echo "$(BLUE)Opening PostgreSQL shell...$(NC)"
	@docker exec -it postgres psql -U $${POSTGRES_USER:-zone} -d $${POSTGRES_DB:-zone}

db-migrate: ## Run database migrations
	@echo "$(BLUE)Running database migrations...$(NC)"
	@for migration in runner/zone_server/migrations/*.sql; do \
		echo "$(GREEN)Applying $$migration...$(NC)"; \
		docker exec -i postgres psql -U $${POSTGRES_USER:-zone} -d $${POSTGRES_DB:-zone} < "$$migration" 2>&1 | grep -v "already exists" || true; \
	done
	@echo "$(GREEN)Migrations complete!$(NC)"

db-reset: ## DANGER: Reset database (requires confirmation)
	@echo "$(RED)WARNING: This will delete ALL database data!$(NC)"
	@read -p "Are you sure? Type 'yes' to confirm: " confirm; \
	if [ "$$confirm" = "yes" ]; then \
		echo "$(RED)Resetting database...$(NC)"; \
		docker exec postgres psql -U $${POSTGRES_USER:-zone} -c "DROP DATABASE IF EXISTS $${POSTGRES_DB:-zone}"; \
		docker exec postgres psql -U $${POSTGRES_USER:-zone} -c "CREATE DATABASE $${POSTGRES_DB:-zone}"; \
		$(MAKE) db-migrate; \
		echo "$(GREEN)Database reset complete!$(NC)"; \
	else \
		echo "$(GREEN)Cancelled.$(NC)"; \
	fi

##@ Maintenance

clean: ## Stop services and remove containers (keeps volumes)
	@echo "$(YELLOW)Cleaning up containers...$(NC)"
	$(DOCKER_COMPOSE) --profile vpn --profile monitoring --profile installer down --remove-orphans
	@echo "$(GREEN)Cleanup complete (volumes preserved)$(NC)"

clean-volumes: ## DANGER: Remove all data volumes (requires confirmation)
	@echo "$(RED)WARNING: This will delete ALL data including models, database, and conversations!$(NC)"
	@read -p "Are you sure? Type 'yes' to confirm: " confirm; \
	if [ "$$confirm" = "yes" ]; then \
		echo "$(RED)Removing volumes...$(NC)"; \
		$(DOCKER_COMPOSE) --profile vpn --profile monitoring --profile installer down -v; \
		echo "$(RED)All data deleted!$(NC)"; \
	else \
		echo "$(GREEN)Cancelled.$(NC)"; \
	fi

prune: ## Remove unused Docker resources
	@echo "$(YELLOW)Pruning Docker resources...$(NC)"
	@docker system prune -f
	@echo "$(GREEN)Prune complete$(NC)"

##@ Backup & Restore

backup: ## Backup volumes to ./backups directory
	@echo "$(BLUE)Creating backup...$(NC)"
	@mkdir -p backups
	@DATE=$$(date +%Y%m%d_%H%M%S); \
	docker run --rm \
		-v zone_ollama_data:/data/ollama:ro \
		-v zone_openwebui_data:/data/openwebui:ro \
		-v zone_postgres_data:/data/postgres:ro \
		-v zone_valkey_data:/data/valkey:ro \
		-v zone_manager_repos:/data/manager_repos:ro \
		-v zone_manager_artifacts:/data/manager_artifacts:ro \
		-v zone_prometheus_data:/data/prometheus:ro \
		-v zone_grafana_data:/data/grafana:ro \
		-v zone_traefik_letsencrypt:/data/traefik:ro \
		-v $$(pwd)/backups:/backup \
		alpine tar czf /backup/zone_backup_$$DATE.tar.gz -C /data .; \
	echo "$(GREEN)Backup created: backups/zone_backup_$$DATE.tar.gz$(NC)"

restore: ## Restore from backup (usage: make restore BACKUP=backups/zone_backup_YYYYMMDD_HHMMSS.tar.gz)
	@if [ -z "$(BACKUP)" ]; then \
		echo "$(RED)Error: Please specify BACKUP file$(NC)"; \
		echo "Usage: make restore BACKUP=backups/zone_backup_20250101_120000.tar.gz"; \
		exit 1; \
	fi
	@echo "$(YELLOW)Restoring from $(BACKUP)...$(NC)"
	@docker run --rm \
		-v zone_ollama_data:/data/ollama \
		-v zone_openwebui_data:/data/openwebui \
		-v zone_postgres_data:/data/postgres \
		-v zone_valkey_data:/data/valkey \
		-v zone_manager_repos:/data/manager_repos \
		-v zone_manager_artifacts:/data/manager_artifacts \
		-v zone_prometheus_data:/data/prometheus \
		-v zone_grafana_data:/data/grafana \
		-v zone_traefik_letsencrypt:/data/traefik \
		-v $$(pwd)/backups:/backup \
		alpine tar xzf /backup/$$(basename $(BACKUP)) -C /data
	@echo "$(GREEN)Restore complete!$(NC)"

##@ Development

dev: ## Start in development mode with live logs
	@echo "$(BLUE)Starting in development mode...$(NC)"
	$(DOCKER_COMPOSE) up

dev-console: ## Start console frontend in development mode
	@echo "$(BLUE)Starting console frontend dev server...$(NC)"
	cd manager/frontend && bun start

dev-installer: ## Start installer frontend in development mode
	@echo "$(BLUE)Starting installer frontend dev server...$(NC)"
	cd installer/frontend && bun start

rebuild: ## Rebuild and restart all services
	@echo "$(BLUE)Rebuilding services...$(NC)"
	$(DOCKER_COMPOSE) up -d --build --force-recreate

rebuild-manager: ## Rebuild only manager and console services
	@echo "$(BLUE)Rebuilding manager and console...$(NC)"
	$(DOCKER_COMPOSE) up -d --build --force-recreate manager console

update: ## Pull latest images and restart
	@echo "$(BLUE)Updating Docker images...$(NC)"
	$(DOCKER_COMPOSE) pull
	$(DOCKER_COMPOSE) up -d --remove-orphans
	@echo "$(GREEN)Update complete!$(NC)"

##@ Shell Access

shell-ollama: ## Open the host Ollama CLI (or a bundled container shell)
	@if docker ps --format '{{.Names}}' | grep -q '^ollama$$'; then \
		docker exec -it ollama /bin/bash; \
	elif command -v ollama >/dev/null 2>&1; then \
		echo "$(GREEN)Host Ollama is the default engine.$(NC)"; \
		ollama list; \
	else \
		echo "$(RED)No Ollama container or host CLI found.$(NC)"; \
		exit 1; \
	fi

shell-litellm: ## Open shell in litellm container
	@docker exec -it litellm /bin/sh

shell-openwebui: ## Open shell in openwebui container
	@docker exec -it openwebui /bin/bash

shell-manager: ## Open shell in manager container
	@docker exec -it manager /bin/sh

shell-console: ## Open shell in console container
	@docker exec -it console /bin/sh

shell-postgres: ## Open shell in postgres container
	@docker exec -it postgres /bin/bash

shell-valkey: ## Open shell in valkey container
	@docker exec -it valkey /bin/sh

##@ Testing

test: ## Run basic smoke tests for all services
	@echo "$(BLUE)Running smoke tests...$(NC)"
	@echo "Testing Ollama API..."
	@curl -sf http://localhost:11434/api/tags >/dev/null 2>&1 && echo "$(GREEN)✓ Ollama$(NC)" || echo "$(RED)✗ Ollama$(NC)"
	@echo "Testing LiteLLM API..."
	@curl -sf http://localhost:4000/health >/dev/null 2>&1 && echo "$(GREEN)✓ LiteLLM$(NC)" || echo "$(RED)✗ LiteLLM$(NC)"
	@echo "Testing Open WebUI..."
	@curl -sf http://localhost:8080/health >/dev/null 2>&1 && echo "$(GREEN)✓ Open WebUI$(NC)" || echo "$(RED)✗ Open WebUI$(NC)"
	@echo "Testing Manager API..."
	@curl -sf http://localhost:8000/api/health >/dev/null 2>&1 && echo "$(GREEN)✓ Manager$(NC)" || echo "$(RED)✗ Manager$(NC)"
	@echo "Testing Console..."
	@curl -sf http://localhost:3000/health >/dev/null 2>&1 && echo "$(GREEN)✓ Console$(NC)" || echo "$(RED)✗ Console$(NC)"

test-server: ## Run zone_server (Rust) unit tests
	@echo "$(BLUE)Running zone_server tests...$(NC)"
	cd runner && cargo test --package zone_server --package zone_core --package zone_cli

build-runner: ## Build the Rust tool runner binary
	@echo "$(BLUE)Building Rust tool runner...$(NC)"
	cd runner && cargo build --release
	@echo "$(GREEN)Runner built: runner/target/release/zone-runner$(NC)"

test-runner: ## Run Rust tool runner tests
	@echo "$(BLUE)Running tool runner tests...$(NC)"
	cd runner && cargo test
	@echo "$(GREEN)Tool runner tests passed!$(NC)"

setup-runner-coverage: ## Install dependencies for Rust code coverage
	@echo "$(BLUE)Setting up Rust code coverage tools...$(NC)"
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		echo "$(YELLOW)Installing cargo-llvm-cov...$(NC)"; \
		cargo install cargo-llvm-cov --version 0.9.0 --locked; \
	fi
	@echo "$(YELLOW)Installing llvm-tools-preview component...$(NC)"
	@rustup component add llvm-tools-preview 2>/dev/null || \
		echo "$(YELLOW)Note: Using system rustup. Run 'rustup default stable && rustup component add llvm-tools-preview' if needed$(NC)"
	@echo "$(GREEN)Coverage tools setup complete!$(NC)"

test-runner-coverage: ## Run tool runner tests with code coverage (requires cargo-llvm-cov)
	@echo "$(BLUE)Running tool runner tests with coverage...$(NC)"
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		echo "$(RED)Error: cargo-llvm-cov not installed. Run 'make setup-runner-coverage' first$(NC)"; \
		exit 1; \
	fi
	@cd runner && cargo llvm-cov --all-features --workspace --exclude zone_desktop --lcov --output-path lcov.info 2>&1 || { \
		echo "$(RED)Coverage failed. Ensure llvm-tools-preview is installed:$(NC)"; \
		echo "  rustup default stable"; \
		echo "  rustup component add llvm-tools-preview"; \
		exit 1; \
	}
	@echo "$(GREEN)Coverage report generated: runner/lcov.info$(NC)"
	@echo ""
	@echo "$(BLUE)Coverage Summary:$(NC)"
	@cd runner && cargo llvm-cov report --lcov --output-path /dev/null 2>/dev/null || true

test-runner-coverage-html: ## Generate HTML coverage report for tool runner
	@echo "$(BLUE)Generating HTML coverage report...$(NC)"
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		echo "$(RED)Error: cargo-llvm-cov not installed. Run 'make setup-runner-coverage' first$(NC)"; \
		exit 1; \
	fi
	@cd runner && cargo llvm-cov --all-features --workspace --exclude zone_desktop --html --output-dir coverage 2>&1 || { \
		echo "$(RED)Coverage failed. Ensure llvm-tools-preview is installed:$(NC)"; \
		echo "  rustup default stable"; \
		echo "  rustup component add llvm-tools-preview"; \
		exit 1; \
	}
	@echo "$(GREEN)HTML coverage report: runner/coverage/html/index.html$(NC)"
	@echo "$(BLUE)Opening in browser...$(NC)"
	@if [ "$$(uname)" = "Darwin" ]; then \
		open runner/coverage/html/index.html; \
	elif [ "$$(uname)" = "Linux" ]; then \
		xdg-open runner/coverage/html/index.html 2>/dev/null || echo "Open runner/coverage/html/index.html in your browser"; \
	fi

test-runner-coverage-json: ## Generate JSON coverage report for tool runner (CI integration)
	@echo "$(BLUE)Generating JSON coverage report...$(NC)"
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		echo "$(RED)Error: cargo-llvm-cov not installed. Run 'make setup-runner-coverage' first$(NC)"; \
		exit 1; \
	fi
	@cd runner && cargo llvm-cov --all-features --workspace --exclude zone_desktop --json --output-path coverage.json 2>&1 || { \
		echo "$(RED)Coverage failed. Ensure llvm-tools-preview is installed:$(NC)"; \
		echo "  rustup default stable"; \
		echo "  rustup component add llvm-tools-preview"; \
		exit 1; \
	}
	@echo "$(GREEN)JSON coverage report: runner/coverage.json$(NC)"

test-runner-coverage-text: ## Show coverage summary in terminal
	@echo "$(BLUE)Running tool runner tests with coverage...$(NC)"
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		echo "$(RED)Error: cargo-llvm-cov not installed. Run 'make setup-runner-coverage' first$(NC)"; \
		exit 1; \
	fi
	@cd runner && cargo llvm-cov --all-features --workspace --exclude zone_desktop 2>&1 || { \
		echo "$(RED)Coverage failed. Ensure llvm-tools-preview is installed:$(NC)"; \
		echo "  rustup default stable"; \
		echo "  rustup component add llvm-tools-preview"; \
		exit 1; \
	}

install-runner: ## Install zone-runner binary to /usr/local/bin
	@echo "$(BLUE)Installing zone-runner...$(NC)"
	@sudo cp runner/target/release/zone-runner /usr/local/bin/
	@echo "$(GREEN)zone-runner installed!$(NC)"

install-cli: ## Install zone CLI to /usr/local/bin
	@echo "$(BLUE)Building and installing zone CLI...$(NC)"
	cd runner && cargo build --release --package zone_cli
	@sudo cp runner/target/release/zone /usr/local/bin/zone
	@echo "$(GREEN)zone CLI installed! Run 'zone --help' to get started.$(NC)"

sqlx-prepare: ## Prepare sqlx offline query data (requires running postgres)
	@echo "$(BLUE)Preparing sqlx offline query data...$(NC)"
	@if docker ps --format '{{.Names}}' | grep -q '^postgres$$'; then \
		cd runner/zone_server && DATABASE_URL="postgresql://$${POSTGRES_USER:-zone}:$${POSTGRES_PASSWORD:-zone}@localhost:5432/$${POSTGRES_DB:-zone}" cargo sqlx prepare; \
		echo "$(GREEN)sqlx offline data prepared!$(NC)"; \
	else \
		echo "$(RED)Error: PostgreSQL container is not running. Start it with 'make up' first.$(NC)"; \
		exit 1; \
	fi

test-console: ## Run console (React) unit tests
	@echo "$(BLUE)Running console unit tests...$(NC)"
	cd manager/frontend && bun test

test-console-coverage: ## Run console tests with coverage report
	@echo "$(BLUE)Running console tests with coverage...$(NC)"
	cd manager/frontend && bun test --coverage

test-e2e: ## Run Playwright end-to-end tests
	@echo "$(BLUE)Running E2E tests...$(NC)"
	cd manager/frontend && bun run test:e2e

test-e2e-ui: ## Run Playwright tests with UI
	@echo "$(BLUE)Running E2E tests with UI...$(NC)"
	cd manager/frontend && bun run test:e2e:ui

test-e2e-headed: ## Run Playwright tests in headed mode
	@echo "$(BLUE)Running E2E tests in headed mode...$(NC)"
	cd manager/frontend && bun run test:e2e:headed

test-all: ## Run all tests (requires 'make up' for server tests)
	@echo "$(BLUE)Running all tests...$(NC)"
	@$(MAKE) dev-test

##@ Code Quality

lint-console: ## Run linter on console code
	@echo "$(BLUE)Linting console code...$(NC)"
	cd manager/frontend && bun run lint

lint-console-fix: ## Run linter and fix issues
	@echo "$(BLUE)Linting and fixing console code...$(NC)"
	cd manager/frontend && bun run lint:fix

format-console: ## Check console code formatting
	@echo "$(BLUE)Checking console code formatting...$(NC)"
	cd manager/frontend && bun run format

format-console-fix: ## Format console code
	@echo "$(BLUE)Formatting console code...$(NC)"
	cd manager/frontend && bun run format:fix

check-console: ## Run all Biome checks on console
	@echo "$(BLUE)Running Biome checks...$(NC)"
	cd manager/frontend && bun run check

check-console-fix: ## Run Biome checks and fix issues
	@echo "$(BLUE)Running Biome checks with fixes...$(NC)"
	cd manager/frontend && bun run check:fix

##@ Information

version: ## Show versions of all components
	@echo "$(BLUE)Component Versions:$(NC)"
	@echo "Docker: $$(docker --version)"
	@echo "Docker Compose: $$($(DOCKER_COMPOSE) version --short 2>/dev/null || $(DOCKER_COMPOSE) --version)"
	@echo ""
	@echo "$(BLUE)Image Versions:$(NC)"
	@$(DOCKER_COMPOSE) config | grep 'image:' | sed 's/image: /  /'

env: ## Show current environment configuration (secrets masked)
	@echo "$(BLUE)Current Configuration:$(NC)"
	@cat .env 2>/dev/null | grep -v '^#' | grep -v '^$$' | sed -E 's/(KEY|PASSWORD|SECRET)=.*/\1=***MASKED***/g' || echo "No .env file found"

urls: ## Show access URLs for services
	@echo "$(BLUE)Service URLs:$(NC)"
	@if [ -f .env ]; then \
		. ./.env; \
		echo "  Web UI:       https://$$DOMAIN_HOST_WEBUI"; \
		echo "  Manager:      https://manager.localhost"; \
		echo "  Manager (alt): https://manager.$$DOMAIN_HOST_WEBUI"; \
		echo "  LiteLLM:      https://litellm.$$DOMAIN_HOST_WEBUI"; \
		echo "  Traefik:      https://traefik.$$DOMAIN_HOST_WEBUI"; \
		echo "  Grafana:      https://grafana.$$DOMAIN_HOST_WEBUI (monitoring profile)"; \
		echo "  Prometheus:   https://prometheus.$$DOMAIN_HOST_WEBUI (monitoring profile)"; \
	else \
		echo "$(RED)No .env file found. Run 'make setup' first.$(NC)"; \
	fi

##@ Dev CLI (zone-dev)

build-dev-cli: ## Build the zone-dev CLI tool
	@echo "$(BLUE)Building zone-dev CLI...$(NC)"
	cd tools/dev-cli && cargo build --release
	@echo "$(GREEN)zone-dev built: tools/dev-cli/target/release/zone-dev$(NC)"

install-dev-cli: build-dev-cli ## Install zone-dev to /usr/local/bin
	@echo "$(BLUE)Installing zone-dev...$(NC)"
	@sudo cp tools/dev-cli/target/release/zone-dev /usr/local/bin/
	@echo "$(GREEN)zone-dev installed! Run 'zone-dev --help' to get started.$(NC)"

dev-format: ## Format all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev format; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev format; \
	fi

dev-format-check: ## Check formatting across all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev format --check; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev format --check; \
	fi

dev-lint: ## Lint all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev lint; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev lint; \
	fi

dev-test: ## Test all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev test; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev test; \
	fi

dev-coverage: ## Coverage for all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev coverage; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev coverage; \
	fi

dev-check: ## Run format check + lint + test on all projects (with TUI)
	@if [ -f tools/dev-cli/target/release/zone-dev ]; then \
		./tools/dev-cli/target/release/zone-dev check; \
	else \
		echo "$(YELLOW)zone-dev not built. Building...$(NC)"; \
		$(MAKE) build-dev-cli; \
		./tools/dev-cli/target/release/zone-dev check; \
	fi

##@ Information

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "$(BLUE)Zone AI Stack - Makefile Commands$(NC)\n\n"} \
		/^[a-zA-Z_-]+:.*?##/ { printf "  $(GREEN)%-25s$(NC) %s\n", $$1, $$2 } \
		/^##@/ { printf "\n$(YELLOW)%s$(NC)\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
