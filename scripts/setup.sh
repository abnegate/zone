#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Voiz Setup Script
# =============================================================================
# This script helps you set up your Voiz AI stack by:
# 1. Checking prerequisites
# 2. Generating secure secrets
# 3. Creating basic auth credentials
# 4. Setting up your .env file
# =============================================================================

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly ENV_FILE="${PROJECT_ROOT}/.env"
readonly ENV_EXAMPLE="${PROJECT_ROOT}/.env.example"
readonly AUTH_DIR="${PROJECT_ROOT}/auth"
readonly AUTH_FILE="${AUTH_DIR}/users.htpasswd"

# Colors
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m'

log_info() {
    printf '%b[INFO]%b %s\n' "${GREEN}" "${NC}" "$1"
}

log_warn() {
    printf '%b[WARN]%b %s\n' "${YELLOW}" "${NC}" "$1"
}

log_error() {
    printf '%b[ERROR]%b %s\n' "${RED}" "${NC}" "$1" >&2
}

log_step() {
    printf '\n%b▶%b %s\n\n' "${BLUE}" "${NC}" "$1"
}

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Generate a secure random string
generate_secret() {
    openssl rand -base64 32
}

# Check prerequisites
check_prerequisites() {
    log_step "Checking prerequisites..."

    local missing=()

    if ! command_exists docker; then
        missing+=("docker")
    fi

    if ! command_exists docker-compose && ! command_exists docker compose; then
        missing+=("docker-compose")
    fi

    if ! command_exists openssl; then
        missing+=("openssl")
    fi

    if ! command_exists htpasswd; then
        log_warn "htpasswd not found. Install apache2-utils (Debian/Ubuntu) or httpd-tools (RHEL/CentOS)"
        missing+=("htpasswd")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        log_error "Missing required commands: ${missing[*]}"
        log_error "Please install them and try again."
        exit 1
    fi

    log_info "✓ All prerequisites met"
}

# Setup .env file
setup_env_file() {
    log_step "Setting up .env file..."

    if [ -f "${ENV_FILE}" ]; then
        read -p "$(echo -e "${YELLOW}.env file already exists. Overwrite? (y/N):${NC} ")" -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "Keeping existing .env file"
            return
        fi
    fi

    if [ ! -f "${ENV_EXAMPLE}" ]; then
        log_error ".env.example not found at ${ENV_EXAMPLE}"
        exit 1
    fi

    log_info "Copying .env.example to .env..."
    cp "${ENV_EXAMPLE}" "${ENV_FILE}"

    log_info "Generating secure secrets..."
    local litellm_key=$(generate_secret)
    local litellm_salt=$(generate_secret)
    local searxng_secret=$(generate_secret)
    local postgres_password=$(generate_secret)

    # Use sed to replace empty values with new prefixed names
    if ! sed -i.bak "s|^SECURITY_LITELLM_MASTER_KEY=.*|SECURITY_LITELLM_MASTER_KEY=${litellm_key}|" "${ENV_FILE}"; then
        log_error "Failed to update SECURITY_LITELLM_MASTER_KEY"
        exit 1
    fi
    if ! sed -i.bak "s|^SECURITY_LITELLM_SALT_KEY=.*|SECURITY_LITELLM_SALT_KEY=${litellm_salt}|" "${ENV_FILE}"; then
        log_error "Failed to update SECURITY_LITELLM_SALT_KEY"
        exit 1
    fi
    if ! sed -i.bak "s|^SECURITY_SEARXNG_SECRET_KEY=.*|SECURITY_SEARXNG_SECRET_KEY=${searxng_secret}|" "${ENV_FILE}"; then
        log_error "Failed to update SECURITY_SEARXNG_SECRET_KEY"
        exit 1
    fi
    if ! sed -i.bak "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=${postgres_password}|" "${ENV_FILE}"; then
        log_error "Failed to update POSTGRES_PASSWORD"
        exit 1
    fi

    # Fix WEBUI_OPENAI_API_KEY to use actual value
    if ! sed -i.bak "s|^WEBUI_OPENAI_API_KEY=.*|WEBUI_OPENAI_API_KEY=${litellm_key}|" "${ENV_FILE}"; then
        log_error "Failed to update WEBUI_OPENAI_API_KEY"
        exit 1
    fi

    # Set WEBUI_CORS_ALLOW_ORIGIN based on DOMAIN_HOST_WEBUI
    local domain_host
    domain_host=$(grep "^DOMAIN_HOST_WEBUI=" "${ENV_FILE}" | cut -d'=' -f2)
    if [ -n "${domain_host}" ]; then
        if ! sed -i.bak "s|^WEBUI_CORS_ALLOW_ORIGIN=.*|WEBUI_CORS_ALLOW_ORIGIN=http://${domain_host}|" "${ENV_FILE}"; then
            log_error "Failed to update WEBUI_CORS_ALLOW_ORIGIN"
            exit 1
        fi
        log_info "Set WEBUI_CORS_ALLOW_ORIGIN to http://${domain_host}"
    fi

    rm -f "${ENV_FILE}.bak"

    log_info "✓ Secrets generated and inserted into .env"
    log_warn "Review ${ENV_FILE} and update:"
    log_warn "  - Domain name (DOMAIN_HOST_WEBUI)"
    log_warn "  - VPN credentials (VPN_OPENVPN_USER, VPN_OPENVPN_PASSWORD)"
    log_warn "  - ACME email (ADVANCED_ACME_EMAIL)"
    log_warn "  - Model choices (OLLAMA_MODEL_*)"
}

# Setup basic auth
setup_basic_auth() {
    log_step "Setting up basic authentication..."

    if [ -f "${AUTH_FILE}" ]; then
        read -p "$(echo -e "${YELLOW}Basic auth file already exists. Overwrite? (y/N):${NC} ")" -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "Keeping existing auth file"
            return
        fi
    fi

    if ! mkdir -p "${AUTH_DIR}" 2>/dev/null; then
        log_error "Failed to create auth directory at ${AUTH_DIR}"
        log_error "Please check permissions and try again"
        exit 1
    fi

    echo -e "${BLUE}Enter username for basic auth:${NC}"
    read -r username

    # Validate username (alphanumeric, dash, underscore only)
    if [[ ! "$username" =~ ^[a-zA-Z0-9_-]+$ ]]; then
        log_error "Invalid username. Use only alphanumeric characters, dash, and underscore"
        exit 1
    fi

    if [ -z "$username" ]; then
        log_error "Username cannot be empty"
        exit 1
    fi

    if ! command_exists htpasswd; then
        log_error "htpasswd not found. Cannot create auth file."
        log_error "Install apache2-utils (Debian/Ubuntu) or httpd-tools (RHEL/CentOS)"
        log_error "Or create it manually: htpasswd -nbB username password > ${AUTH_FILE}"
        exit 1
    fi

    # Create or overwrite htpasswd file
    htpasswd -cB "${AUTH_FILE}" "${username}"

    log_info "✓ Basic auth configured for user: ${username}"
    log_info "Auth file location: ${AUTH_FILE}"
}

# Add more users to basic auth
add_auth_user() {
    log_step "Adding additional user to basic auth..."

    if [ ! -f "${AUTH_FILE}" ]; then
        log_error "Auth file doesn't exist. Run setup first."
        exit 1
    fi

    echo -e "${BLUE}Enter username to add:${NC}"
    read -r username

    # Validate username (alphanumeric, dash, underscore only)
    if [[ ! "$username" =~ ^[a-zA-Z0-9_-]+$ ]]; then
        log_error "Invalid username. Use only alphanumeric characters, dash, and underscore"
        exit 1
    fi

    if [ -z "$username" ]; then
        log_error "Username cannot be empty"
        exit 1
    fi

    # Append without -c flag
    htpasswd -B "${AUTH_FILE}" "${username}"

    log_info "✓ User added: ${username}"
}

# Validate configuration
validate_config() {
    log_step "Validating configuration..."

    if [ ! -f "${ENV_FILE}" ]; then
        log_error ".env file not found. Run setup first."
        exit 1
    fi

    if [ ! -f "${AUTH_FILE}" ]; then
        log_error "Auth file not found. Run setup first."
        exit 1
    fi

    # Source .env file safely (export vars without executing)
    set -a
    # Use grep to filter out comments and empty lines, then source
    while IFS= read -r line; do
        # Skip comments and empty lines
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ -z "$line" ]] && continue

        # Extract key (before first =)
        key="${line%%=*}"
        # Trim whitespace from key (safely without command substitution)
        key="${key#"${key%%[![:space:]]*}"}"  # Remove leading whitespace
        key="${key%"${key##*[![:space:]]}"}"  # Remove trailing whitespace

        # Skip if no valid key
        [[ -z "$key" ]] && continue

        # Extract value (everything after first =)
        value="${line#*=}"

        # Remove quotes from value (both single and double)
        if [[ "$value" =~ ^\"(.*)\"$ ]] || [[ "$value" =~ ^\'(.*)\'$ ]]; then
            value="${BASH_REMATCH[1]}"
        fi

        # Only export valid variable names (alphanumeric and underscore)
        if [[ "$key" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ ]]; then
            export "$key=$value"
        fi
    done < <(grep -v '^\s*#' "${ENV_FILE}" | grep -v '^\s*$')
    set +a

    local errors=0

    # Check for insecure default values
    if [[ "${SECURITY_LITELLM_MASTER_KEY:-}" == *"dev-insecure"* ]]; then
        log_warn "SECURITY_LITELLM_MASTER_KEY is using default insecure value (OK for dev, change for production)"
    fi

    if [[ "${SECURITY_SEARXNG_SECRET_KEY:-}" == *"dev-insecure"* ]]; then
        log_warn "SECURITY_SEARXNG_SECRET_KEY is using default insecure value (OK for dev, change for production)"
    fi

    # Check VPN credentials based on VPN_TYPE
    if [[ "${VPN_TYPE:-openvpn}" == "openvpn" ]]; then
        if [[ "${VPN_OPENVPN_USER:-}" == "" ]]; then
            log_warn "VPN_OPENVPN_USER not set (OpenVPN will not work)"
        fi
        if [[ "${VPN_OPENVPN_PASSWORD:-}" == "" ]]; then
            log_warn "VPN_OPENVPN_PASSWORD not set (OpenVPN will not work)"
        fi
    elif [[ "${VPN_TYPE:-}" == "wireguard" ]]; then
        if [[ "${VPN_WIREGUARD_PRIVATE_KEY:-}" == "" ]]; then
            log_warn "VPN_WIREGUARD_PRIVATE_KEY not set (WireGuard will not work)"
        fi
        if [[ "${VPN_WIREGUARD_ADDRESSES:-}" == "" ]]; then
            log_warn "VPN_WIREGUARD_ADDRESSES not set (WireGuard will not work)"
        fi
    fi

    if [ $errors -gt 0 ]; then
        log_error "Configuration validation failed with $errors error(s)"
        exit 1
    fi

    log_info "✓ Configuration looks good!"
}

# Print next steps
print_next_steps() {
    echo -e "\n${GREEN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}Setup Complete!${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}\n"

    echo -e "Next steps:"
    echo -e ""
    echo -e "  1. Review your .env file:"
    echo -e "     ${BLUE}nano ${ENV_FILE}${NC}"
    echo -e ""
    echo -e "  2. Update VPN credentials (if using VPN):"
    echo -e "     ${BLUE}VPN_OPENVPN_USER${NC} and ${BLUE}VPN_OPENVPN_PASSWORD${NC}"
    echo -e ""
    echo -e "  3. Update domain name for your setup:"
    echo -e "     ${BLUE}DOMAIN_HOST_WEBUI${NC}"
    echo -e ""
    echo -e "  4. Start the stack:"
    echo -e "     ${BLUE}make up${NC} or ${BLUE}docker compose up -d${NC}"
    echo -e ""
    echo -e "  5. Check logs:"
    echo -e "     ${BLUE}make logs${NC} or ${BLUE}docker compose logs -f${NC}"
    echo -e ""
    echo -e "  6. Access the web UI:"
    echo -e "     ${BLUE}https://webui.localhost${NC}"
    echo -e ""
    echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}\n"
}

# Main menu
main_menu() {
    echo -e "\n${BLUE}╔════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║      Voiz Setup Script                 ║${NC}"
    echo -e "${BLUE}╚════════════════════════════════════════╝${NC}\n"

    echo "1) Full setup (recommended for first-time setup)"
    echo "2) Generate secrets only"
    echo "3) Setup basic auth only"
    echo "4) Add auth user"
    echo "5) Validate configuration"
    echo "6) Exit"
    echo ""
    read -p "Select option [1-6]: " choice

    case $choice in
        1)
            check_prerequisites
            setup_env_file
            setup_basic_auth
            validate_config
            print_next_steps
            ;;
        2)
            setup_env_file
            ;;
        3)
            setup_basic_auth
            ;;
        4)
            add_auth_user
            ;;
        5)
            validate_config
            ;;
        6)
            log_info "Exiting..."
            exit 0
            ;;
        *)
            log_error "Invalid option"
            exit 1
            ;;
    esac
}

# Run main menu
main_menu
