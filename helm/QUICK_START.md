# Zone Helm + Kind Quick Start

Zone ships three charts:

| Chart | What it deploys |
|-------|-----------------|
| `helm/zone-infra` | CloudNativePG PostgreSQL + Valkey |
| `helm/zone-ai` | Ollama + LiteLLM |
| `helm/zone-apps` | zone-server (Rust API), manager console, Open WebUI |

## Kind development

```bash
make kind-create
make tilt-up
```

`make kind-create` starts a `zone-dev` cluster, installs the CloudNativePG operator and HAProxy ingress, and uses Kind's built-in local-path storage.

`make tilt-up` writes `k8s/secrets.yaml` (gitignored) and starts Tilt. The console runs locally on http://localhost:3001; the API is forwarded to http://localhost:8000.

```bash
make tilt-down      # stop Tilt, keep the cluster
make kind-delete    # delete the cluster
```

## Helm-only install

Prerequisites: a cluster with the CloudNativePG operator, a default StorageClass, and kubectl context set.

```bash
kubectl create namespace zone

# Application + CNPG owner credentials (username/password are required by CNPG)
kubectl -n zone create secret generic zone-secrets \
  --from-literal=username=zone \
  --from-literal=password="$POSTGRES_PASSWORD" \
  --from-literal=postgres-password="$POSTGRES_PASSWORD" \
  --from-literal=valkey-password="$VALKEY_PASSWORD" \
  --from-literal=jwt-secret="$JWT_SECRET" \
  --from-literal=encryption-key="$ENCRYPTION_KEY" \
  --from-literal=litellm-master-key="$LITELLM_MASTER_KEY" \
  --from-literal=litellm-salt-key="$LITELLM_SALT_KEY" \
  --from-literal=database-url="postgres://zone:${POSTGRES_PASSWORD}@zone-postgres-rw:5432/manager" \
  --from-literal=litellm-database-url="postgres://zone:${POSTGRES_PASSWORD}@zone-postgres-rw:5432/litellm" \
  --from-literal=valkey-url="redis://:${VALKEY_PASSWORD}@valkey:6379"

kubectl -n zone create secret generic zone-postgres-superuser \
  --from-literal=username=postgres \
  --from-literal=password="$POSTGRES_SUPERUSER_PASSWORD" \
  --type=kubernetes.io/basic-auth

helm upgrade --install zone-infra ./helm/zone-infra --namespace zone
helm upgrade --install zone-ai ./helm/zone-ai --namespace zone
helm upgrade --install zone-apps ./helm/zone-apps --namespace zone \
  -f helm/zone-apps/examples/development.yaml
```

`JWT_SECRET` and `ENCRYPTION_KEY` must be at least 32 characters. zone-server runs sqlx migrations on startup; do not enable `migration.enabled`.

## Access

With Kind + HAProxy, add `127.0.0.1 zone.local` to `/etc/hosts` and open http://zone.local (ingress class `haproxy`). If host port 80 is already taken (for example by `make up`), Kind maps ingress to http://127.0.0.1:8080 instead.

Without ingress:

```bash
kubectl -n zone port-forward svc/zone-apps-server 8000:8000
kubectl -n zone port-forward svc/zone-apps-manager 3001:3001
```

## Troubleshooting

```bash
kubectl -n zone get pods,cluster,svc
kubectl -n zone logs -l app.kubernetes.io/component=server
kubectl -n cnpg-system get pods
```

The PostgreSQL service is `zone-postgres-rw`, not `zone-postgres`.
