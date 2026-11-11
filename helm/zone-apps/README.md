# Zone Apps Helm Chart

Production-ready Kubernetes Helm chart for deploying the Zone platform, including the Rust backend server and React frontend manager.

## Prerequisites

- Kubernetes 1.19+
- Helm 3.2.0+
- PostgreSQL database (in-cluster or external)

## Installation

### Quick Start

```bash
# Add the repository (if published)
helm repo add zone https://charts.zone.dev
helm repo update

# Install with default values
helm install zone zone/zone-apps

# Install with custom values
helm install zone zone/zone-apps -f values.yaml
```

### Local Installation

```bash
# From the helm directory
helm install zone ./zone-apps

# With custom values
helm install zone ./zone-apps -f custom-values.yaml
```

## Configuration

The following table lists the configurable parameters and their default values.

### Global Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `global.imageRegistry` | Global Docker image registry | `""` |
| `global.imagePullSecrets` | Global Docker registry secret names | `[]` |
| `global.externalDatabase` | Use external database | `false` |
| `global.securityContexts` | Enable security contexts | `true` |

### Server Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `server.enabled` | Enable server deployment | `true` |
| `server.replicaCount` | Number of server replicas | `2` |
| `server.image.repository` | Server image repository | `zone/server` |
| `server.image.tag` | Server image tag | `latest` |
| `server.service.type` | Kubernetes service type | `ClusterIP` |
| `server.service.port` | Service port | `8000` |
| `server.autoscaling.enabled` | Enable horizontal pod autoscaler | `true` |
| `server.autoscaling.minReplicas` | Minimum number of replicas | `2` |
| `server.autoscaling.maxReplicas` | Maximum number of replicas | `10` |
| `server.autoscaling.targetCPUUtilization` | Target CPU utilization percentage | `70` |
| `server.baseUrl` | Origin a browser reaches Zone at, `scheme://host[:port]` with no path, passed to zone-server as `APP_BASE_URL`; see [Public Origin](#public-origin) | `""` (the first ingress host) |
| `server.agentState.persistence.enabled` | Keep agent state (each sign-in's CLI home under `<state>/<org>/<agent>/logins/<id>`, with codex's login and the CLI's session files, and each agent's shared `work` directory) on a claim; applies only with one replica and autoscaling off. Claude tokens are in the database either way | `true` |
| `server.agentState.persistence.size` | Size of the agent-state claim, which grows with every CLI turn; see [Coding Agent Sign-ins](#coding-agent-sign-ins) | `1Gi` |
| `server.agentState.persistence.storageClass` | Storage class of the agent-state claim; empty uses the cluster default | `""` |
| `server.env.ZONE_AGENT_USAGE_TTL_SECONDS` | Whole seconds a sign-in's usage reading stays fresh before zone-server reads it again to pick the account a chat or task starts on; anything else is refused at boot | `"60"` |
| `server.env.ZONE_CLAUDE_API_URL` | Where zone-server reads Claude sign-ins' usage and profile, for example through a proxy | unset (`https://api.anthropic.com`) |
| `server.env.ZONE_CODEX_API_URL` | Where zone-server reads codex sign-ins' usage, for example through a proxy | unset (`https://chatgpt.com`) |
| `server.env.ZONE_MCP_ENABLED` | Master switch for every MCP server, instance and organization alike; see [docs/CONFIGURATION.md](../../docs/CONFIGURATION.md) | `"true"` |
| `server.env.ZONE_MCP_AUTO_CONNECT` | Attach `magents mcp` when no enabled instance server is configured and `magents` is on `PATH`; the image does not ship it | `"true"` |
| `server.env.ZONE_MCP_AUTO_MAGENTS` | The older name for `ZONE_MCP_AUTO_CONNECT`; either one off keeps the fallback off | `"true"` |
| `server.env.ZONE_MCP_COMMANDS` | Let organization admins add stdio (command) servers, which run as zone-server's user in the pod | `"false"` |
| `server.env.ZONE_MCP_CONFIG` | Path inside the pod to a JSON file of instance MCP servers | `""` |
| `server.env.ZONE_MCP_SERVERS` | Inline JSON of instance MCP servers; takes precedence over `ZONE_MCP_CONFIG` | `""` |

### Manager Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `manager.enabled` | Enable manager deployment | `true` |
| `manager.replicaCount` | Number of manager replicas | `2` |
| `manager.image.repository` | Manager image repository | `zone/manager` |
| `manager.image.tag` | Manager image tag | `latest` |
| `manager.service.type` | Kubernetes service type | `ClusterIP` |
| `manager.service.port` | Service port | `3001` |

### Database Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `commonEnv.DB_HOST` | Database host | `zone-postgres-rw` |
| `commonEnv.DB_PORT` | Database port | `5432` |
| `commonEnv.DB_NAME` | Database name | `manager` |
| `commonEnv.DB_USER` | Database user | `zone` |
| `secrets.create` | Create a chart-managed Secret | `false` |
| `secrets.existingSecret` | Existing Secret name | `zone-secrets` |

### Ingress Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `ingress.enabled` | Enable ingress | `true` |
| `ingress.className` | Ingress class name | `haproxy` |
| `ingress.hosts[0].host` | Hostname | `zone.local` |

### Migration Job

| Parameter | Description | Default |
|-----------|-------------|---------|
| `migration.enabled` | Unused; zone-server migrates on boot | `false` |
| `migration.backoffLimit` | Job backoff limit | `0` |
| `migration.activeDeadlineSeconds` | Job timeout in seconds | `600` |

## Examples

### Production Deployment

```yaml
# production-values.yaml
environment: production

global:
  externalDatabase: true
  imagePullSecrets:
    - name: docker-registry-secret

secrets:
  dbPassword: "strong-password-here"
  jwtSecret: "jwt-secret-here"
  encryptionKey: "encryption-key-here"

server:
  image:
    repository: gcr.io/my-project/zone-apps
    tag: "1.0.0"
  autoscaling:
    minReplicas: 3
    maxReplicas: 20
  resources:
    limits:
      cpu: 2000m
      memory: 2Gi
    requests:
      cpu: 1000m
      memory: 1Gi

manager:
  image:
    repository: gcr.io/my-project/zone-manager
    tag: "1.0.0"
  autoscaling:
    minReplicas: 3
    maxReplicas: 10

ingress:
  enabled: true
  className: nginx
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
    nginx.ingress.kubernetes.io/ssl-redirect: "true"
  hosts:
    - host: zone.example.com
      paths:
        - path: /api
          pathType: Prefix
          service: server
        - path: /
          pathType: Prefix
          service: manager
  tls:
    - secretName: zone-tls
      hosts:
        - zone.example.com
```

Install:
```bash
helm install zone ./zone-apps -f production-values.yaml
```

### Development Deployment

```yaml
# dev-values.yaml
environment: development

server:
  replicaCount: 1
  autoscaling:
    enabled: false
  podDisruptionBudget:
    enabled: false

manager:
  replicaCount: 1
  autoscaling:
    enabled: false
  podDisruptionBudget:
    enabled: false

ingress:
  enabled: true
  hosts:
    - host: zone.localhost
      paths:
        - path: /api
          pathType: Prefix
          service: server
        - path: /
          pathType: Prefix
          service: manager
```

Install:
```bash
helm install zone ./zone-apps -f dev-values.yaml
```

### External Database Configuration

```yaml
# external-db-values.yaml
global:
  externalDatabase: true

commonEnv:
  DB_HOST: "postgres.example.com"
  DB_PORT: "5432"
  DB_NAME: "zone_production"
  DB_USER: "zone_user"

secrets:
  dbPassword: "production-db-password"

migration:
  enabled: true
```

## Public Origin

zone-server builds every outward link from `APP_BASE_URL`: the callbacks of the GitHub App that **Connect GitHub** registers and the page a browser lands on afterwards, the address GitHub delivers repository webhooks to, and the links in sign-up verification and password reset mail. The chart sets it on the server container, from the first of these that has a value:

1. `server.env.APP_BASE_URL`
2. `server.baseUrl`
3. `commonEnv.APP_BASE_URL`
4. the first `ingress.hosts` entry, while `ingress.enabled` is true: `https://<host>` when an `ingress.tls` entry lists that host, lists a wildcard one label up from it (`*.example.com` for `zone.example.com`) or lists no hosts at all, and `http://<host>` otherwise. The shipped values give `http://zone.local`.

With none of the first three set and no ingress host to go by (the ingress disabled, or its first host empty or a wildcard), the chart leaves `APP_BASE_URL` unset and zone-server falls back to `http://localhost:3000`. A named value that is not an origin, such as one with a path or no scheme, fails the render.

Set `server.baseUrl` when the derived origin is not the one browsers use: TLS that ends at a load balancer or proxy in front of the ingress, a port other than 80 or 443, or an install reached without the ingress, such as through `kubectl port-forward`:

```yaml
server:
  baseUrl: https://zone.example.com
```

GitHub keeps the addresses an App was registered with, so settle the origin before connecting GitHub. After changing it, correct the addresses in the App's settings on GitHub, or delete the App and connect again.

## Upgrading

```bash
# Upgrade to a new version
helm upgrade zone ./zone-apps -f values.yaml

# Upgrade with force migration
helm upgrade zone ./zone-apps -f values.yaml --set migration.enabled=true
```

## Uninstalling

```bash
helm uninstall zone
```

## Security Considerations

1. **Secrets Management**: Always use strong, randomly generated secrets in production. Consider using external secret management solutions like:
   - HashiCorp Vault
   - AWS Secrets Manager
   - Azure Key Vault
   - Kubernetes External Secrets Operator

2. **Database Credentials**: Never commit database passwords to version control. Use Helm values files with `.gitignore` or external secret providers.

3. **Security Contexts**: The chart applies restrictive security contexts by default:
   - Non-root user (UID 1000)
   - Read-only root filesystem
   - Dropped capabilities
   - Seccomp profile

4. **Network Policies**: Consider enabling network policies in your cluster to restrict pod-to-pod communication.

5. **TLS**: Always enable TLS in production environments using cert-manager or manual certificate management.

## High Availability

The chart is configured for high availability by default:

- **Multiple Replicas**: Server and Manager run with 2 replicas minimum
- **Pod Disruption Budgets**: Ensures at least 1 pod is available during disruptions
- **Anti-Affinity Rules**: Spreads pods across different nodes
- **Topology Spread Constraints**: Ensures even distribution across the cluster
- **Horizontal Pod Autoscaler**: Automatically scales based on CPU utilization
- **Health Checks**: Liveness and readiness probes ensure traffic only goes to healthy pods

Organizations on the Claude Code or Codex provider need a single server replica instead; see [Coding Agent Sign-ins](#coding-agent-sign-ins).

Each server replica runs its own copy of an organization's MCP servers. A change made through one replica reaches the others the next time they list that organization's servers or run a turn for it.

## Coding Agent Sign-ins

zone-server can run the claude and codex CLIs its image ships for organizations that choose the Claude Code or Codex provider, as described under Model Backend in [docs/CONFIGURATION.md](../../docs/CONFIGURATION.md). The chart prepares the server pod for them:

- `server.env.ZONE_AGENT_HOST_LOGIN` is `"false"`, so an organization that has not signed in gets an error rather than a login made inside the pod.
- `ZONE_AGENT_CALLBACK` stays unset, so admins paste the code claude.com shows. Its loopback sign-in, where claude.com hands the code to `http://localhost:<port>/callback` on the admin's own computer, reaches a pod only through `kubectl port-forward` of that port and of the console, with the console opened at a localhost address. The install notes say so when `server.env` sets it.
- `server.env.ZONE_CONSOLE_ORIGINS` is empty, so no sign-in returns to the callback even when it is set. It lists the consoles a returned sign-in goes on to, comma separated, each exactly as the browser opens it. With the callback set and the console forwarded as the install notes show, list the forwarded console, `ZONE_CONSOLE_ORIGINS: "http://127.0.0.1:3001"`, or `http://localhost:3001` if that is the address you open. List only consoles you run: the callback sends the approving browser, with its sign-in's receipt, to whichever listed console started the sign-in.
- The server container sets `HOME=/home/zone`, `ZONE_AGENT_STATE_DIR=/app/agent-state` and `ZONE_CHAT_AGENT_CWD=/app/workspace`. The root filesystem is read-only, so `/home/zone`, `/app/workspace` and `/tmp` are emptyDirs. Set `server.hostRoot` to a host directory to bind-mount it at `/host` and pass `ZONE_HOST_ROOT` / `ZONE_IN_CONTAINER` so workspace Folders can map host paths into the pod.
- `/app/agent-state` is a ReadWriteOnce claim, set by `server.agentState.persistence`, only when `server.replicaCount` is `1` and `server.autoscaling.enabled` is `false`. The Deployment then uses the `Recreate` strategy. Otherwise it is an emptyDir, and the install notes say what that loses.
- `/app/clones` is a `clones` directory on the same volume, with `ZONE_PROJECT_CLONE_DIR` pointed at it. A project chat whose repository is not checked out in one of its workspace's Folders clones it there, so size the claim for those repositories too. On an emptyDir, clones and any work left in them go with the pod.
- An organization can sign in to each agent with several accounts. Each sign-in gets its own CLI home, `/app/agent-state/<org>/<agent>/logins/<id>`, which holds codex's login and the CLI's session files: claude's under `projects/`, codex's under `sessions/`. Every sign-in of an agent works in the same `/app/agent-state/<org>/<agent>/work`, so a chat that moves to another account keeps its files, and its session file moves with it.
- `server.env.ZONE_AGENT_USAGE_TTL_SECONDS` is `"60"`: zone-server reads an account's usage again once its last reading is older than that, and starts each chat or task on the account with the most room left. `ZONE_CLAUDE_API_URL` and `ZONE_CODEX_API_URL` are unset, so those reads go to `https://api.anthropic.com` and `https://chatgpt.com`.

The claim holds codex's logins and both CLIs' state; Claude sign-ins are kept in the database. With an emptyDir, zone-server loses codex logins whenever the pod is replaced (a rollout, an eviction or a reschedule).

Agent providers need a single server replica. A Claude sign-in waiting for its code and a codex sign-in in progress live in one server process's memory, and a codex login lives on the pod that made it. With several replicas a sign-in can start on one pod and fail on another, and a turn on another pod finds no codex login. The chart's defaults run two replicas with autoscaling, so set:

```yaml
server:
  replicaCount: 1
  autoscaling:
    enabled: false
  podDisruptionBudget:
    enabled: false
```

The disruption budget has to go as well: with one replica, its `minAvailable: 1` would stop a node drain from ever evicting the pod.

Helm 4 upgrades a release it installed server-side, and server-side apply cannot remove the `rollingUpdate` settings Kubernetes filled in for a Deployment an earlier version of this chart created. If the upgrade that moves such a release to one replica fails with `spec.strategy.rollingUpdate: Forbidden`, run it again with `--server-side=false`.

The claim carries `helm.sh/resource-policy: keep`. Neither `helm uninstall` nor an upgrade that stops using it, such as one to a second replica, deletes it, and a release of the same name mounts it again once it is back to one replica. The claim holds working ChatGPT logins, so delete it by hand once they should go: for a release named `zone`, `kubectl delete pvc zone-zone-apps-agent-state`.

Size the claim for transcripts. Every claude and codex turn, whether a chat message, a task attempt, a title or a review, writes a transcript of its own under its organization's directory, and Zone prunes none of them, so the claim grows with use. Check it with `kubectl exec deploy/zone-zone-apps-server -- du -sh /app/agent-state`. Before it fills, raise `server.agentState.persistence.size`, where the claim's storage class allows volume expansion, or delete old transcripts from each sign-in's `projects` (claude) and `sessions` (codex) directories under `<org>/<agent>/logins/<id>`. On a full claim neither CLI can write its transcript, and codex cannot save a login it has renewed.

The image sets `ZONE_CODEX_SANDBOX=danger-full-access`: codex sandboxes its own shell with bubblewrap, which needs user namespaces, and Docker's default seccomp profile blocks them. Whether the pods' `RuntimeDefault` profile allows them has not been tested; set `server.env.ZONE_CODEX_SANDBOX: workspace-write` only where it does.

With `networkPolicy.enabled`, the server's egress policy admits DNS, the database, Valkey and LiteLLM only, so neither the CLIs nor zone-server's own Claude token exchange can reach Anthropic or OpenAI. zone-server's usage reads need `api.anthropic.com` and `chatgpt.com` (or the hosts `ZONE_CLAUDE_API_URL` and `ZONE_CODEX_API_URL` name) as well; without them every account's usage reads as unknown.

Every organization's CLI runs as the pod's user, uid 1000 with the chart's default security context. Read the security notes under Model Backend in [docs/CONFIGURATION.md](../../docs/CONFIGURATION.md) before enabling these providers on a shared instance.

## Monitoring

The chart includes Prometheus annotations for metrics scraping:

```yaml
prometheus.io/scrape: "true"
prometheus.io/port: "8080"
prometheus.io/path: "/metrics"
```

Ensure your Prometheus instance is configured to scrape pods with these annotations.

## Troubleshooting

### Migration Job Fails

```bash
# Check migration job logs
kubectl logs -l app.kubernetes.io/component=migration

# Check if database is accessible
kubectl run -it --rm debug --image=postgres:15-alpine --restart=Never -- \
  psql -h zone-postgres-rw -p 5432 -U zone -d manager
```

### Pods Not Starting

```bash
# Check pod status
kubectl get pods -l app.kubernetes.io/name=zone-apps

# Describe pod for events
kubectl describe pod <pod-name>

# Check logs
kubectl logs <pod-name>
```

### Database Connection Issues

```bash
# Test database connectivity from a pod
kubectl exec -it <server-pod> -- sh
# Inside pod:
# Check if database hostname resolves
# Verify credentials in secrets
```

## License

Copyright (c) 2024 Zone Team

## Support

For issues and questions:
- GitHub Issues: https://github.com/yourusername/zone/issues
- Documentation: https://docs.zone.dev
