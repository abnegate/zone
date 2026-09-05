# VPN Setup Guide

## VPN is Optional

The Zone AI stack works perfectly **without VPN**. You only need VPN if you want:
- Private, anonymous web search
- All stack internet traffic to leave through the tunnel
- A kill switch if the VPN drops

## Running Without VPN (Default)

```bash
sh scripts/configure-model-proxy.sh .env direct
MODEL_SEARCH_PROXY_URL= TOOL_RUNNER_PROXY_URL= ZONE_VPN= docker compose -f docker-compose.yml up -d
# or
make up
```

**What works**: Everything except web search
- ✅ Chat with local models
- ✅ Semantic routing (auto/fast/reason)
- ✅ All core functionality
- ❌ Web search (SearXNG not available — Zone chat)

## Running With VPN

### Step 1: Configure VPN Credentials

Edit `.env` and add your VPN credentials:

```bash
VPN_OPENVPN_USER=your_surfshark_username
VPN_OPENVPN_PASSWORD=your_surfshark_password
```

### Step 2: Start with VPN Profile

```bash
make up-vpn
```

The VPN launch writes `ZONE_VPN=1`, `MODEL_SEARCH_PROXY_URL=http://gluetun:8888`,
and `TOOL_RUNNER_PROXY_URL=http://gluetun:8888` to `.env`, then starts Compose
with `docker-compose.vpn.yml`. Internet-facing services share Gluetun's network
namespace, so all of their IP traffic uses the tunnel and Gluetun's kill switch:

- SearXNG, Manager, LiteLLM, Grafana
- Bundled Ollama / ComfyUI and ComfyUI model downloads
- Traefik ACME (HTTP proxy to Gluetun)
- Tool and model-catalog HTTP (proxy env as belt-and-suspenders)

Postgres, Valkey, Prometheus, and the console stay on Docker networks so the
local UI and databases keep working. Rebuilds preserve `ZONE_VPN=1`, so later
`make rebuild`, `make up-comfyui`, and model-download targets keep the overlay.

For direct Compose usage:

```bash
sh scripts/configure-model-proxy.sh .env vpn
docker compose -f docker-compose.yml -f docker-compose.vpn.yml --profile vpn up -d
```

The overlay requires Docker Compose v2.24+ (`!reset` / `!override`).

When disabling the VPN, run `make down && make up`. The direct launch clears
`ZONE_VPN` and both proxy URLs. When a proxy URL is configured, an unavailable
proxy causes proxy-aware HTTP requests to fail; they do not retry directly.
Internal service and loopback destinations bypass the proxy.

**What works**: Everything including web search
- ✅ Chat with local models
- ✅ Semantic routing
- ✅ All core functionality
- ✅ Private web search via VPN (Zone chat, through SearXNG on Gluetun)
- ✅ All other stack internet traffic through the same tunnel

**Still on the host network** (not the container VPN):
- Host Ollama (`OLLAMA_BASE_URL=http://host.docker.internal:11434`)
- Host ComfyUI
- The machine's own browser and other host processes

## Supported VPN Providers

Gluetun supports many VPN providers. See [Gluetun Wiki](https://github.com/qdm12/gluetun-wiki) for complete list:

- Surfshark (default in this project)
- NordVPN
- ExpressVPN
- ProtonVPN
- Mullvad
- Private Internet Access (PIA)
- And 30+ more...

### Changing VPN Provider

Edit `.env` for OpenVPN:

```bash
VPN_SERVICE_PROVIDER=nordvpn
VPN_TYPE=openvpn
VPN_OPENVPN_USER=your_username
VPN_OPENVPN_PASSWORD=your_password
```

Or for WireGuard (faster, more modern):

```bash
VPN_SERVICE_PROVIDER=mullvad
VPN_TYPE=wireguard
VPN_WIREGUARD_PRIVATE_KEY=your_private_key
VPN_WIREGUARD_ADDRESSES=10.x.x.x/32
```

### OpenVPN vs WireGuard

**OpenVPN** (default):
- ✅ Widely supported by all providers
- ✅ Simple username/password authentication
- ⚠️ Slower (more overhead)
- ⚠️ More complex protocol

**WireGuard** (recommended if available):
- ✅ Much faster (less overhead)
- ✅ Modern cryptography
- ✅ Better battery life on mobile
- ⚠️ Requires config file from provider
- ⚠️ Not all providers support it yet

### Getting WireGuard Configuration

Most providers offer WireGuard configs in their account dashboard:

**Mullvad**:
1. Login to account
2. Download WireGuard config
3. Extract: `PrivateKey`, `Address`, `PresharedKey`

**Surfshark**:
1. Account → Manual Setup → WireGuard
2. Generate credentials
3. Extract keys from config file

**NordVPN**:
1. Account → NordLynx (WireGuard)
2. Download configuration
3. Extract credentials

## Troubleshooting VPN

### VPN Won't Connect

```bash
# Check Gluetun logs
docker logs gluetun

# Common issues:
# 1. Wrong username/password
# 2. VPN provider API changes
# 3. Firewall blocking VPN
```

### Confirm traffic uses the tunnel

```bash
# Egress IP from the VPN namespace (must be the VPN, not your ISP)
docker exec gluetun wget -qO- https://ifconfig.me

# Same check from Manager (shares Gluetun's network when VPN is on)
docker exec manager wget -qO- https://ifconfig.me
```

If the VPN drops, Gluetun's firewall blocks outbound internet from attached
containers. Docker/LAN subnets in `FIREWALL_OUTBOUND_SUBNETS` stay reachable so
Manager can still talk to Postgres and Valkey.

### Web Search Not Working with VPN

```bash
# Check if SearXNG is running
docker compose -f docker-compose.yml -f docker-compose.vpn.yml --profile vpn ps

# Check if Gluetun is healthy
docker inspect gluetun | grep Health

# Test VPN connection
docker exec gluetun wget -qO- ifconfig.me
```

### Disable VPN Temporarily

```bash
# Stop only VPN services
docker compose --profile vpn down

# Start without VPN
sh scripts/configure-model-proxy.sh .env direct
MODEL_SEARCH_PROXY_URL= TOOL_RUNNER_PROXY_URL= ZONE_VPN= docker compose -f docker-compose.yml up -d
# or
make down && make up
```

## Performance Impact

- VPN adds 20-100ms latency to outbound internet requests
- No impact on local model inference
- Bandwidth limited by VPN server location

## Privacy Considerations

**With VPN**:
- Search, catalogs, cloud providers, alerts, and bundled pulls use the tunnel
- IP address masked from those destinations
- Location privacy protected
- Kill switch: no internet from attached containers if the tunnel is down

**Without VPN**:
- No web search capability (SearXNG not running)
- Remote model catalogs and tool HTTP requests use their normal direct route
- Local inference remains local
