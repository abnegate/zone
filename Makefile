.PHONY: help setup up down restart logs logs-follow ps health check \
	pull-models clean clean-volumes backup restore \
	setup-auth add-user validate test

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

up: ## Start all services (without VPN)
	@echo "$(GREEN)Starting services...$(NC)"
	$(DOCKER_COMPOSE) up -d
	@echo "$(GREEN)Services started! Check status with: make ps$(NC)"
	@echo "$(YELLOW)Note: VPN not enabled. For VPN-protected search, use: make up-vpn$(NC)"

up-vpn: ## Start all services with VPN-protected search
	@echo "$(GREEN)Starting services with VPN...$(NC)"
	$(DOCKER_COMPOSE) --profile vpn up -d
	@echo "$(GREEN)Services started with VPN! Check status with: make ps$(NC)"

down: ## Stop all services
	@echo "$(YELLOW)Stopping services...$(NC)"
	$(DOCKER_COMPOSE) down

restart: ## Restart all services
	@echo "$(YELLOW)Restarting services...$(NC)"
	$(DOCKER_COMPOSE) restart

ps: ## Show service status
	@$(DOCKER_COMPOSE) ps

logs: ## Show recent logs (non-following)
	@$(DOCKER_COMPOSE) logs --tail=100

logs-follow: ## Follow logs from all services
	@$(DOCKER_COMPOSE) logs -f

logs-service: ## Follow logs for a specific service (usage: make logs-service SERVICE=ollama)
	@$(DOCKER_COMPOSE) logs -f $(SERVICE)

##@ Health & Monitoring

health: ## Check health status of all services
	@echo "$(BLUE)Service Health Status:$(NC)"
	@$(DOCKER_COMPOSE) ps --format json | jq -r '.[] | "\(.Name): \(.Health)"' 2>/dev/null || \
	$(DOCKER_COMPOSE) ps | grep -E '(Up|Exited|Restarting)'

check: health ## Alias for health check

stats: ## Show resource usage
	@docker stats --no-stream --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}"

##@ Model Management

pull-models: ## Manually trigger model pulling
	@echo "$(BLUE)Pulling Ollama models...$(NC)"
	@$(DOCKER_COMPOSE) run --rm ollama-init

list-models: ## List downloaded Ollama models
	@echo "$(BLUE)Downloaded Ollama models:$(NC)"
	@docker exec ollama ollama list

##@ Maintenance

clean: ## Stop services and remove containers (keeps volumes)
	@echo "$(YELLOW)Cleaning up containers...$(NC)"
	$(DOCKER_COMPOSE) down --remove-orphans
	@echo "$(GREEN)Cleanup complete (volumes preserved)$(NC)"

clean-volumes: ## DANGER: Remove all data volumes (requires confirmation)
	@echo "$(RED)WARNING: This will delete ALL data including models and conversations!$(NC)"
	@read -p "Are you sure? Type 'yes' to confirm: " confirm; \
	if [ "$$confirm" = "yes" ]; then \
		echo "$(RED)Removing volumes...$(NC)"; \
		$(DOCKER_COMPOSE) down -v; \
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
		-v voiz_ollama_data:/data/ollama:ro \
		-v voiz_openwebui_data:/data/openwebui:ro \
		-v $$(pwd)/backups:/backup \
		alpine tar czf /backup/voiz_backup_$$DATE.tar.gz -C /data .; \
	echo "$(GREEN)Backup created: backups/voiz_backup_$$DATE.tar.gz$(NC)"

restore: ## Restore from backup (usage: make restore BACKUP=backups/voiz_backup_YYYYMMDD_HHMMSS.tar.gz)
	@if [ -z "$(BACKUP)" ]; then \
		echo "$(RED)Error: Please specify BACKUP file$(NC)"; \
		echo "Usage: make restore BACKUP=backups/voiz_backup_20250101_120000.tar.gz"; \
		exit 1; \
	fi
	@echo "$(YELLOW)Restoring from $(BACKUP)...$(NC)"
	@docker run --rm \
		-v voiz_ollama_data:/data/ollama \
		-v voiz_openwebui_data:/data/openwebui \
		-v $$(pwd)/backups:/backup \
		alpine tar xzf /backup/$$(basename $(BACKUP)) -C /data
	@echo "$(GREEN)Restore complete!$(NC)"

##@ Development

dev: ## Start in development mode with live logs
	@echo "$(BLUE)Starting in development mode...$(NC)"
	$(DOCKER_COMPOSE) up

rebuild: ## Rebuild and restart all services
	@echo "$(BLUE)Rebuilding services...$(NC)"
	$(DOCKER_COMPOSE) up -d --build --force-recreate

update: ## Pull latest images and restart
	@echo "$(BLUE)Updating Docker images...$(NC)"
	$(DOCKER_COMPOSE) pull
	$(DOCKER_COMPOSE) up -d --remove-orphans
	@echo "$(GREEN)Update complete!$(NC)"

shell-ollama: ## Open shell in ollama container
	@docker exec -it ollama /bin/bash

shell-litellm: ## Open shell in litellm container
	@docker exec -it litellm /bin/sh

shell-openwebui: ## Open shell in openwebui container
	@docker exec -it openwebui /bin/bash

##@ Testing

test: ## Run basic smoke tests
	@echo "$(BLUE)Running smoke tests...$(NC)"
	@echo "Testing Ollama API..."
	@curl -f http://localhost:11434/api/tags >/dev/null 2>&1 && echo "$(GREEN)✓ Ollama$(NC)" || echo "$(RED)✗ Ollama$(NC)"
	@echo "Testing LiteLLM API..."
	@curl -f http://localhost:4000/health >/dev/null 2>&1 && echo "$(GREEN)✓ LiteLLM$(NC)" || echo "$(RED)✗ LiteLLM$(NC)"
	@echo "Testing Open WebUI..."
	@curl -f http://localhost:8080/health >/dev/null 2>&1 && echo "$(GREEN)✓ Open WebUI$(NC)" || echo "$(RED)✗ Open WebUI$(NC)"

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
		echo "  Web UI:     https://$$WEBUI_HOST"; \
		echo "  API:        https://$$API_HOST"; \
		echo "  Search:     https://$$SEARX_HOST (internal)"; \
		echo "  Traefik:    https://traefik.$$WEBUI_HOST"; \
	else \
		echo "$(RED)No .env file found. Run 'make setup' first.$(NC)"; \
	fi

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "$(BLUE)Voiz AI Stack - Makefile Commands$(NC)\n\n"} \
		/^[a-zA-Z_-]+:.*?##/ { printf "  $(GREEN)%-20s$(NC) %s\n", $$1, $$2 } \
		/^##@/ { printf "\n$(YELLOW)%s$(NC)\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
