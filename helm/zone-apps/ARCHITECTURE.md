# Zone Helm Chart Architecture

This document describes the architecture and design decisions of the Zone Helm chart.

## Overview

The Zone Helm chart deploys a production-ready, highly available platform consisting of:
- **Server**: Rust-based backend API
- **Manager**: React-based frontend application
- **Migration Job**: Database schema migration (pre-install/upgrade hook)

## Architecture Diagram

```
                                    ┌─────────────┐
                                    │   Ingress   │
                                    └──────┬──────┘
                                           │
                        ┌──────────────────┼──────────────────┐
                        │                  │                  │
                   /api │                  │ /                │
                        │                  │                  │
                 ┌──────▼──────┐    ┌──────▼──────┐          │
                 │   Server    │    │   Manager   │          │
                 │  Service    │    │   Service   │          │
                 └──────┬──────┘    └──────┬──────┘          │
                        │                  │                  │
         ┌──────────────┴────────┐  ┌──────┴─────────┐       │
         │                       │  │                │       │
    ┌────▼────┐            ┌────▼──▼───┐      ┌────▼────┐  │
    │ Server  │    ...     │  Server    │      │ Manager │  ...
    │  Pod 1  │            │   Pod N    │      │  Pod 1  │
    └────┬────┘            └────┬───────┘      └────┬────┘
         │                      │                   │
         └──────────────────────┴───────────────────┘
                                │
                         ┌──────▼──────┐
                         │  PostgreSQL │
                         │  Database   │
                         └─────────────┘
```

## Components

### 1. Server Deployment

**Purpose**: Handles all backend API requests, business logic, and database interactions.

**Key Features**:
- Multiple replicas (default: 2) for high availability
- Horizontal Pod Autoscaler (HPA) for automatic scaling
- Pod Disruption Budget (PDB) to ensure availability during updates
- Init container to wait for database availability
- Health checks (liveness and readiness probes)
- Security contexts (non-root user, read-only filesystem)
- Anti-affinity rules to spread pods across nodes

**Resources**:
- Default: 500m CPU / 512Mi RAM requests
- Limits: 1000m CPU / 1Gi RAM

### 2. Manager Deployment

**Purpose**: Serves the React frontend application to users.

**Key Features**:
- Multiple replicas (default: 2) for high availability
- HPA for automatic scaling based on traffic
- PDB for zero-downtime deployments
- Static file serving with nginx
- Security contexts

**Resources**:
- Default: 250m CPU / 256Mi RAM requests
- Limits: 500m CPU / 512Mi RAM

### 3. Migration Job

**Purpose**: Applies database schema migrations before deployments.

**Key Features**:
- Runs as a Helm hook (pre-install, pre-upgrade)
- Ensures database schema is up-to-date before app deployment
- Fails fast (backoffLimit: 0) to prevent deployments with migration issues
- Direct database connection (bypasses connection poolers)
- Automatic cleanup after completion

**Execution**:
1. Helm starts migration job
2. Init container waits for database
3. Migration runs
4. If successful, deployment proceeds
5. If failed, deployment is blocked

### 4. Service Account

**Purpose**: Provides identity for pods within the cluster.

**Features**:
- Custom service account per deployment
- Can be configured with additional annotations (e.g., for workload identity)
- Used by all pods for cluster API access

### 5. ConfigMap

**Purpose**: Stores non-sensitive configuration data.

**Contents**:
- Database connection details (host, port, name, user)
- Logging configuration
- Environment settings

**Why ConfigMap**:
- Allows configuration updates without rebuilding images
- Can be mounted as files or environment variables
- Shareable across multiple pods

### 6. Secret

**Purpose**: Stores sensitive data securely.

**Contents**:
- Database password
- JWT secret
- Encryption key
- Optional: Stripe key, SMTP password

**Security**:
- Base64 encoded by default
- Should be encrypted at rest in production
- Consider using external secret management (Vault, AWS Secrets Manager)

### 7. Ingress

**Purpose**: Routes external traffic to internal services.

**Features**:
- Path-based routing (/api → server, / → manager)
- TLS termination support
- Configurable annotations for ingress controller
- Support for multiple ingress controllers (nginx, HAProxy, etc.)

## High Availability Design

### Replica Distribution

```
Node 1                  Node 2                  Node 3
┌──────────────┐       ┌──────────────┐       ┌──────────────┐
│ Server Pod 1 │       │ Server Pod 2 │       │ Server Pod 3 │
│ Manager Pod 1│       │ Manager Pod 2│       │              │
└──────────────┘       └──────────────┘       └──────────────┘
```

**Anti-Affinity**: Ensures pods are distributed across nodes
**Topology Spread**: Maintains even distribution
**PDB**: Ensures minimum pods available during disruptions

### Autoscaling Strategy

```
Traffic         │                    ┌─────────┐
                │                ┌───┤ Scale Up│
                │            ┌───┤   └─────────┘
                │        ┌───┤   │
Low ────────────┼────────┤   │   │
                │        │   │   │   ┌─────────┐
                │        │   │   └───┤Scale Down│
High ───────────┼────────┘   └───────┤         │
                │                    └─────────┘
                └────────────────────────────────► Time
```

**Scale Up**: Fast (50% increase, max 2 pods per minute)
**Scale Down**: Slow (10% decrease, 5-minute stabilization)

## Security Model

### Defense in Depth

```
┌─────────────────────────────────────────────────────┐
│ Network Layer: Ingress, Network Policies           │
├─────────────────────────────────────────────────────┤
│ Pod Layer: Security Contexts, Service Accounts     │
├─────────────────────────────────────────────────────┤
│ Container Layer: Non-root, Read-only FS, Dropped   │
│                  capabilities                       │
├─────────────────────────────────────────────────────┤
│ Application Layer: Secrets, Encryption, Auth       │
└─────────────────────────────────────────────────────┘
```

### Security Contexts

**Pod Security Context**:
- `runAsNonRoot: true` - Prevents root execution
- `runAsUser: 1000` - Specific non-root UID
- `fsGroup: 2000` - File system group
- `seccompProfile: RuntimeDefault` - Syscall filtering

**Container Security Context**:
- `allowPrivilegeEscalation: false` - Prevents privilege escalation
- `capabilities.drop: [ALL]` - Removes all Linux capabilities
- `readOnlyRootFilesystem: true` - Immutable container filesystem

### Secret Management

**Options**:
1. **Helm Values** (default): Simple but less secure
2. **External Secrets Operator**: Syncs from external providers
3. **Sealed Secrets**: Encrypted secrets in Git
4. **Vault**: Full secret lifecycle management

## Resource Management

### Resource Allocation Strategy

```yaml
Server:
  Requests: 500m CPU, 512Mi RAM  # Guaranteed resources
  Limits:   1000m CPU, 1Gi RAM   # Maximum resources

Manager:
  Requests: 250m CPU, 256Mi RAM
  Limits:   500m CPU, 512Mi RAM
```

**Requests**: Kubernetes schedules based on these
**Limits**: OOM killer triggered if exceeded

### QoS Classes

Based on resource configuration:
- **Guaranteed**: requests == limits (best priority)
- **Burstable**: requests < limits (medium priority)
- **BestEffort**: no requests/limits (lowest priority)

Default configuration: **Burstable** (allows some flexibility)

## Deployment Flow

### Initial Installation

```
1. helm install zone zone-apps
2. ├─ Create namespace (if doesn't exist)
3. ├─ Create ServiceAccount
4. ├─ Create ConfigMap & Secret
5. ├─ Run Migration Job (pre-install hook)
6. │  ├─ Wait for database
7. │  ├─ Apply migrations
8. │  └─ Verify success
9. ├─ Create Server Deployment
10. │  ├─ Wait for database
11. │  └─ Start server pods
12. ├─ Create Manager Deployment
13. │  └─ Start manager pods
14. ├─ Create Services
15. ├─ Create Ingress
16. ├─ Create HPA
17. └─ Create PDB
```

### Upgrade Process

```
1. helm upgrade zone zone-apps
2. ├─ Run Migration Job (pre-upgrade hook)
3. │  └─ Apply new migrations
4. ├─ Update ConfigMap & Secret
5. ├─ Rolling update Server Deployment
6. │  ├─ Create new pod
7. │  ├─ Wait for readiness
8. │  ├─ Terminate old pod
9. │  └─ Repeat (respecting PDB)
10. └─ Rolling update Manager Deployment
```

## Configuration Patterns

### Environment-Based Configuration

```yaml
# Base values.yaml
commonEnv:
  DB_HOST: "postgres"
  RUST_LOG: "info"

# Override for production
commonEnv:
  DB_HOST: "postgres.prod.svc"
  RUST_LOG: "warn,zone=info"
```

### Feature Flags

```yaml
# Enable/disable components
server:
  enabled: true

manager:
  enabled: true

migration:
  enabled: true

ingress:
  enabled: true
```

### Multi-Environment Support

```
values.yaml              # Base configuration
├─ development.yaml      # Dev overrides
├─ staging.yaml          # Staging overrides
└─ production.yaml       # Prod overrides
```

## Scaling Strategies

### Vertical Scaling

Increase resources per pod:
```yaml
server:
  resources:
    requests:
      cpu: 2000m
      memory: 2Gi
```

### Horizontal Scaling

Increase pod count:
```yaml
server:
  replicaCount: 10
  autoscaling:
    minReplicas: 10
    maxReplicas: 50
```

### Database Scaling

For production workloads:
- Use external managed database (RDS, Cloud SQL)
- Enable read replicas
- Use connection pooling (PgBouncer)

## Monitoring & Observability

### Metrics Collection

```yaml
annotations:
  prometheus.io/scrape: "true"
  prometheus.io/port: "8080"
  prometheus.io/path: "/metrics"
```

### Health Checks

**Liveness Probe**: Restarts unhealthy containers
**Readiness Probe**: Removes unhealthy pods from service

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: http
  initialDelaySeconds: 30
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /ready
    port: http
  initialDelaySeconds: 5
  periodSeconds: 5
```

## Best Practices Implemented

1. **Security First**: Restrictive security contexts by default
2. **High Availability**: Multiple replicas, PDB, anti-affinity
3. **Resource Efficiency**: Appropriate requests/limits
4. **Zero Downtime**: Rolling updates with readiness checks
5. **Fast Failure**: Migration job fails fast
6. **Observability**: Health checks, Prometheus annotations
7. **Flexibility**: Feature flags, environment overrides
8. **Documentation**: Comprehensive README and examples

## Future Enhancements

Potential additions:
- Network policies for pod-to-pod security
- Service mesh integration (Istio/Linkerd)
- Custom metrics for autoscaling
- Backup/restore jobs
- Multi-region deployment support
- Blue/green deployment support
- Canary deployment support

## References

- [Kubernetes Best Practices](https://kubernetes.io/docs/concepts/configuration/overview/)
- [Helm Best Practices](https://helm.sh/docs/chart_best_practices/)
- [12-Factor App](https://12factor.net/)
- [Production Best Practices](https://kubernetes.io/docs/setup/best-practices/)
