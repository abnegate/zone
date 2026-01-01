# VPN Setup Guide

## VPN is Optional

The Zone AI stack works perfectly **without VPN**. You only need VPN if you want:
- Private, anonymous web search
- IP masking for search queries
- Enhanced privacy when using web search features

## Running Without VPN (Default)

```bash
docker compose up -d
# or
make up
```

**What works**: Everything except web search
- ✅ Chat with local models
- ✅ Semantic routing (auto/fast/reason)
- ✅ All core functionality
- ❌ Web search (SearXNG not available)

## Running With VPN

### Step 1: Configure VPN Credentials

Edit `.env` and add your VPN credentials:

```bash
OPENVPN_USER=your_surfshark_username
OPENVPN_PASSWORD=your_surfshark_password
```

### Step 2: Start with VPN Profile

```bash
docker compose --profile vpn up -d
# or
make up-vpn
```

**What works**: Everything including web search
- ✅ Chat with local models
- ✅ Semantic routing
- ✅ All core functionality
- ✅ Private web search via VPN

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

### Web Search Not Working with VPN

```bash
# Check if SearXNG is running
docker compose --profile vpn ps

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
docker compose up -d
```

## Performance Impact

- VPN adds 20-100ms latency to search queries
- No impact on local model inference
- Bandwidth limited by VPN server location

## Privacy Considerations

**With VPN**:
- Search queries go through encrypted VPN tunnel
- IP address masked from search engines
- Location privacy protected

**Without VPN**:
- No web search capability (SearXNG not running)
- All other features are local and private
- No telemetry or external connections (except model downloads)
