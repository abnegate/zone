# Zone Helm Chart - Quick Start Guide

This guide will help you quickly deploy Zone to your Kubernetes cluster.

## Prerequisites

1. **Kubernetes Cluster**: A running Kubernetes cluster (1.19+)
2. **Helm**: Helm 3.2.0 or later installed
3. **kubectl**: Configured to access your cluster
4. **PostgreSQL**: Either an external database or in-cluster PostgreSQL

## Quick Deploy (5 minutes)

### Step 1: Verify Prerequisites

```bash
# Check Helm version
helm version

# Check kubectl access
kubectl cluster-info

# Check namespace
kubectl get namespace default
```

### Step 2: Clone and Navigate

```bash
cd /Users/jakebarnby/Local/zone/helm
```

### Step 3: Create Secrets

Create a secrets file (don't commit this!):

```bash
cat > my-secrets.yaml <<EOF
secrets:
  dbPassword: "$(openssl rand -base64 32)"
  jwtSecret: "$(openssl rand -base64 32)"
  encryptionKey: "$(openssl rand -base64 32)"
EOF
```

### Step 4: Deploy

Choose your deployment scenario:

#### Option A: Development Deployment

```bash
helm install zone zone-server \
  -f zone-server/examples/development.yaml \
  -f my-secrets.yaml
```

#### Option B: Production Deployment

```bash
# Edit production values first
cp zone-server/examples/production.yaml my-production.yaml
# Edit my-production.yaml with your values

helm install zone zone-server \
  -f my-production.yaml \
  -f my-secrets.yaml
```

#### Option C: Minimal Testing

```bash
helm install zone zone-server \
  -f zone-server/examples/minimal.yaml \
  -f my-secrets.yaml
```

### Step 5: Verify Deployment

```bash
# Check pod status
kubectl get pods -l app.kubernetes.io/instance=zone

# Check migration job
kubectl get jobs -l app.kubernetes.io/component=migration

# Check services
kubectl get svc -l app.kubernetes.io/instance=zone
```

### Step 6: Access the Application

#### If Ingress is Enabled:

```bash
# Get ingress details
kubectl get ingress

# Add to /etc/hosts if needed
echo "127.0.0.1 zone.local" | sudo tee -a /etc/hosts

# Access the application
open http://zone.local
```

#### If Ingress is Disabled (Port Forward):

```bash
# Port forward server
kubectl port-forward svc/zone-zone-server-server 8080:8080 &

# Port forward manager
kubectl port-forward svc/zone-zone-server-manager 3000:3000 &

# Access the application
open http://localhost:3000
```

## Common Scenarios

### Deploy with External Database

```yaml
# external-db.yaml
global:
  externalDatabase: true

commonEnv:
  DB_HOST: "postgres.example.com"
  DB_PORT: "5432"
  DB_NAME: "zone_prod"
  DB_USER: "zone_user"

secrets:
  dbPassword: "your-secure-password"
```

Deploy:
```bash
helm install zone zone-server -f external-db.yaml
```

### Enable Only Server (No Manager)

```yaml
# server-only.yaml
server:
  enabled: true

manager:
  enabled: false

ingress:
  enabled: true
  hosts:
    - host: api.zone.com
      paths:
        - path: /
          pathType: Prefix
          service: server
```

Deploy:
```bash
helm install zone zone-server -f server-only.yaml
```

### Scale Up Replicas

```bash
# Scale server to 5 replicas
helm upgrade zone zone-server \
  --set server.replicaCount=5 \
  --reuse-values

# Scale manager to 3 replicas
helm upgrade zone zone-server \
  --set manager.replicaCount=3 \
  --reuse-values
```

### Update Image Tags

```bash
# Update server image
helm upgrade zone zone-server \
  --set server.image.tag=v1.2.3 \
  --reuse-values

# Update both
helm upgrade zone zone-server \
  --set server.image.tag=v1.2.3 \
  --set manager.image.tag=v1.2.3 \
  --reuse-values
```

## Monitoring Deployment

### Watch Pods

```bash
watch kubectl get pods -l app.kubernetes.io/instance=zone
```

### Check Logs

```bash
# Server logs
kubectl logs -l app.kubernetes.io/component=server -f

# Manager logs
kubectl logs -l app.kubernetes.io/component=manager -f

# Migration logs
kubectl logs -l app.kubernetes.io/component=migration
```

### Check HPA

```bash
# View autoscaling status
kubectl get hpa

# Watch autoscaling
watch kubectl get hpa
```

### Check Resource Usage

```bash
# View resource usage
kubectl top pods -l app.kubernetes.io/instance=zone
```

## Troubleshooting

### Pods Not Starting

```bash
# Describe pod
kubectl describe pod <pod-name>

# Check events
kubectl get events --sort-by='.lastTimestamp'

# Check init containers
kubectl logs <pod-name> -c wait-for-database
```

### Migration Failed

```bash
# Check migration job
kubectl get jobs

# View migration logs
kubectl logs -l app.kubernetes.io/component=migration

# Delete and retry
kubectl delete job -l app.kubernetes.io/component=migration
helm upgrade zone zone-server --reuse-values
```

### Database Connection Issues

```bash
# Test database from a pod
kubectl run -it --rm debug --image=postgres:15-alpine --restart=Never -- \
  psql -h <DB_HOST> -p <DB_PORT> -U <DB_USER> -d <DB_NAME>

# Check secrets
kubectl get secret zone-zone-server-secrets -o yaml
```

### Image Pull Errors

```bash
# Check image pull secrets
kubectl get secrets

# Create image pull secret
kubectl create secret docker-registry docker-registry-secret \
  --docker-server=gcr.io \
  --docker-username=_json_key \
  --docker-password="$(cat key.json)"

# Update deployment
helm upgrade zone zone-server \
  --set global.imagePullSecrets[0].name=docker-registry-secret \
  --reuse-values
```

## Cleanup

### Uninstall

```bash
# Uninstall release
helm uninstall zone

# Verify cleanup
kubectl get all -l app.kubernetes.io/instance=zone

# Clean up secrets (if needed)
kubectl delete secret zone-zone-server-secrets
```

### Delete Namespace

```bash
# If deployed to dedicated namespace
kubectl delete namespace zone
```

## Next Steps

1. **Configure Ingress**: Set up proper domain and TLS certificates
2. **Set up Monitoring**: Configure Prometheus and Grafana
3. **Configure Backup**: Set up database backups
4. **Review Security**: Ensure all secrets are properly managed
5. **Load Testing**: Test autoscaling behavior
6. **Disaster Recovery**: Document and test recovery procedures

## Best Practices

1. **Never commit secrets** to version control
2. **Use external secret management** (Vault, AWS Secrets Manager, etc.)
3. **Enable TLS** for production deployments
4. **Configure proper resource limits** based on your workload
5. **Enable monitoring** and alerting
6. **Regular backups** of your database
7. **Test upgrades** in staging before production
8. **Use specific image tags** instead of `latest` in production

## Getting Help

- Chart README: `/Users/jakebarnby/Local/zone/helm/zone-server/README.md`
- Examples: `/Users/jakebarnby/Local/zone/helm/zone-server/examples/`
- Helm documentation: https://helm.sh/docs/

## Support

For issues and questions:
- GitHub Issues: https://github.com/yourusername/zone/issues
- Documentation: https://docs.zone.dev
