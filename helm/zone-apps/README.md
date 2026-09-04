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

manager:
  replicaCount: 1
  autoscaling:
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
