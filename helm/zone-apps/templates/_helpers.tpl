{{/*
Expand the name of the chart.
*/}}
{{- define "zone-apps.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "zone-apps.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "zone-apps.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "zone-apps.labels" -}}
helm.sh/chart: {{ include "zone-apps.chart" . }}
{{ include "zone-apps.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "zone-apps.selectorLabels" -}}
app.kubernetes.io/name: {{ include "zone-apps.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Server specific labels
*/}}
{{- define "zone-apps.server.labels" -}}
{{ include "zone-apps.labels" . }}
app.kubernetes.io/component: server
{{- end }}

{{/*
Server selector labels
*/}}
{{- define "zone-apps.server.selectorLabels" -}}
{{ include "zone-apps.selectorLabels" . }}
app.kubernetes.io/component: server
{{- end }}

{{/*
Claim holding zone-server's agent state, or nothing. A ReadWriteOnce claim
holds per-pod state, so it is used only while the server runs a single
replica. A ReadWriteMany claim is shared by every replica, however many run,
and has its own name because a claim's access modes cannot change.
*/}}
{{- define "zone-apps.server.agentStateClaim" -}}
{{- $persistence := dig "agentState" "persistence" dict .Values.server -}}
{{- $single := and (not .Values.server.autoscaling.enabled) (eq (int .Values.server.replicaCount) 1) -}}
{{- if include "zone-apps.server.sharedState" . -}}
{{ include "zone-apps.fullname" . }}-shared-state
{{- else if and .Values.server.enabled $persistence.enabled $single -}}
{{ include "zone-apps.fullname" . }}-agent-state
{{- end -}}
{{- end }}

{{/*
"true" when the agent-state claim is ReadWriteMany, so every replica mounts
agent state, clones, artifacts and the chat workspace from it; otherwise
nothing.
*/}}
{{- define "zone-apps.server.sharedState" -}}
{{- $persistence := dig "agentState" "persistence" dict .Values.server -}}
{{- if and .Values.server.enabled $persistence.enabled (eq ($persistence.accessMode | default "ReadWriteOnce") "ReadWriteMany") -}}
true
{{- end -}}
{{- end }}

{{/*
Probe rendered from a server.probes entry, in a fixed field order.
*/}}
{{- define "zone-apps.probe" -}}
httpGet:
  path: {{ .path }}
  port: http
{{- if not (kindIs "invalid" .initialDelaySeconds) }}
initialDelaySeconds: {{ .initialDelaySeconds }}
{{- end }}
periodSeconds: {{ .periodSeconds }}
timeoutSeconds: {{ .timeoutSeconds }}
failureThreshold: {{ .failureThreshold }}
{{- end }}

{{/*
secrets.redisUrl, refused unless its scheme is redis or rediss.
*/}}
{{- define "zone-apps.secrets.redisUrl" -}}
{{- $url := .Values.secrets.redisUrl | default "" | toString -}}
{{- if $url -}}
{{- if not (has (urlParse $url).scheme (list "redis" "rediss")) -}}
{{- fail "secrets.redisUrl must start with redis:// or rediss://" -}}
{{- end -}}
{{- $url -}}
{{- end -}}
{{- end }}

{{/*
Fails a highAvailability.enabled render whose replicas would not share one
external, managed Redis or Valkey, that leaves the server's trusted proxies
unset, or that trusts a network no network policy closes to other pods. helm
template cannot read an existing secret or the cluster's own policies, so the
operator vouches for those with highAvailability.externalValkey and
highAvailability.externalNetworkPolicy.
*/}}
{{- define "zone-apps.highAvailability.validate" -}}
{{- if .Values.secrets.create -}}
{{- $url := include "zone-apps.secrets.redisUrl" . -}}
{{- if not $url -}}
{{- fail "highAvailability.enabled needs an external Redis or Valkey: set secrets.redisUrl to its redis:// or rediss:// URL" -}}
{{- end -}}
{{- $hostname := regexReplaceAll ":[0-9]*$" (urlParse $url).host "" | trimPrefix "[" | trimSuffix "]" | trimSuffix "." | lower -}}
{{- if not $hostname -}}
{{- fail "secrets.redisUrl must name the host of the external Redis or Valkey" -}}
{{- end -}}
{{- if or (regexMatch "^valkey(\\.[^.]+(\\.svc(\\..+)?)?)?$" $hostname) (regexMatch "^[^.]+\\.valkey\\.[^.]+\\.svc(\\..+)?$" $hostname) -}}
{{- fail (printf "highAvailability.enabled needs an external Redis or Valkey, but secrets.redisUrl names the in-cluster valkey service (%s)" $hostname) -}}
{{- end -}}
{{- else if not .Values.highAvailability.externalValkey -}}
{{- fail (printf "highAvailability.enabled needs an external Redis or Valkey: point key %s of secrets.existingSecret at it and set highAvailability.externalValkey=true, or set secrets.create with secrets.redisUrl" (include "zone-apps.secretKey" (dict "root" . "name" "redisUrl" "default" "valkey-url"))) -}}
{{- end -}}
{{- if not (include "zone-apps.server.trustedProxies" .) -}}
{{- fail "highAvailability.enabled needs server.trustedProxies, or zone-server sees every client at the ingress controller's address and throttles their single sign-on as one: set it to your ingress controller's pod CIDR, such as 10.244.0.0/16 on kind, with networkPolicy.enabled or highAvailability.externalNetworkPolicy, or to 127.0.0.1/32,::1/128 to trust loopback only (escape the comma as \\, in --set)" -}}
{{- end -}}
{{- $networks := include "zone-apps.server.trustedNetworks" . -}}
{{- if and $networks (not .Values.networkPolicy.enabled) (not .Values.highAvailability.externalNetworkPolicy) -}}
{{- fail (printf "server.trustedProxies trusts the network %s, and on most CNIs every pod in it can reach zone-server directly and forge X-Forwarded-For, evading the per-address single sign-on and SCIM limits and choosing the address recorded on its sessions and devices: set networkPolicy.enabled so only the ingress controller's namespace (networkPolicy.ingressNamespace) reaches the server, or enforce a NetworkPolicy of your own that does and set highAvailability.externalNetworkPolicy=true" $networks) -}}
{{- end -}}
{{- end }}

{{/*
Fails a render that gives more than one zone-server replica one
ZONE_RUNNER_NAME: of the live servers sharing a runner name, only the newest
takes work under an Enterprise license.
*/}}
{{- define "zone-apps.server.runnerName.validate" -}}
{{- if or .Values.highAvailability.enabled .Values.server.autoscaling.enabled (gt (int .Values.server.replicaCount) 1) -}}
{{- range $source, $env := dict "commonEnv" .Values.commonEnv "server.env" .Values.server.env -}}
{{- if get (default dict $env) "ZONE_RUNNER_NAME" | default "" | toString | trim -}}
{{- fail (printf "%s.ZONE_RUNNER_NAME gives every zone-server replica one runner name, so under an Enterprise license only the newest replica takes work: unset it and each pod registers under its own pod name" $source) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end }}

{{/*
Manager specific labels
*/}}
{{- define "zone-apps.manager.labels" -}}
{{ include "zone-apps.labels" . }}
app.kubernetes.io/component: manager
{{- end }}

{{/*
Manager selector labels
*/}}
{{- define "zone-apps.manager.selectorLabels" -}}
{{ include "zone-apps.selectorLabels" . }}
app.kubernetes.io/component: manager
{{- end }}

{{/*
Under highAvailability, a scheduling term's label selector also matches the
release, so two releases in one namespace never count each other's pods. A
term without a selector matches no pods and is left alone.
*/}}
{{- define "zone-apps.releaseSelector" -}}
{{- if kindIs "map" .term.labelSelector }}
{{- $labels := default (dict) .term.labelSelector.matchLabels }}
{{- $_ := set .term.labelSelector "matchLabels" (merge (dict "app.kubernetes.io/instance" .context.Release.Name) $labels) }}
{{- end }}
{{- end }}

{{- define "zone-apps.topologySpreadConstraints" -}}
{{- $constraints := deepCopy .constraints }}
{{- if .context.Values.highAvailability.enabled }}
{{- range $constraints }}
{{- include "zone-apps.releaseSelector" (dict "term" . "context" $.context) }}
{{- end }}
{{- end }}
{{- toYaml $constraints }}
{{- end }}

{{- define "zone-apps.podAntiAffinity" -}}
{{- $affinity := deepCopy .affinity }}
{{- if .context.Values.highAvailability.enabled }}
{{- range $affinity.preferredDuringSchedulingIgnoredDuringExecution }}
{{- include "zone-apps.releaseSelector" (dict "term" (dig "podAffinityTerm" (dict) .) "context" $.context) }}
{{- end }}
{{- range $affinity.requiredDuringSchedulingIgnoredDuringExecution }}
{{- include "zone-apps.releaseSelector" (dict "term" . "context" $.context) }}
{{- end }}
{{- end }}
{{- toYaml $affinity }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "zone-apps.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "zone-apps.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Image name helper
*/}}
{{- define "zone-apps.image" -}}
{{- $repo   := required "image repository required" .repository -}}
{{- $tag    := .tag | default "latest" -}}
{{- $digest := .digest | default "" -}}
{{- $registry := .Values.global.imageRegistry | default "" -}}
{{- if $digest }}
{{- printf "%s%s@%s" $registry $repo $digest -}}
{{- else }}
{{- printf "%s%s:%s" $registry $repo $tag -}}
{{- end -}}
{{- end -}}

{{/*
Common environment variables for all containers
*/}}
{{- define "zone-apps.commonEnv" -}}
{{- $skipped := list "APP_BASE_URL" "REDIS_URL" -}}
{{- if .Values.highAvailability.enabled -}}
{{- $skipped = append $skipped "ZONE_RUNNER_NAME" -}}
{{- end -}}
{{- range $key, $value := .Values.commonEnv }}
{{- if not (has $key $skipped) }}
- name: {{ $key }}
  value: {{ $value | quote }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Origin a browser reaches Zone at, or nothing. A named value wins, server.env
over server.baseUrl over commonEnv; otherwise it is the first ingress host.
*/}}
{{- define "zone-apps.server.baseUrl" -}}
{{- $named := coalesce (get (default dict .Values.server.env) "APP_BASE_URL") .Values.server.baseUrl (get (default dict .Values.commonEnv) "APP_BASE_URL") | default "" | toString -}}
{{- $host := "" -}}
{{- if and .Values.ingress.enabled .Values.ingress.hosts -}}
{{- $host = (first .Values.ingress.hosts).host | default "" -}}
{{- end -}}
{{- if $named -}}
{{- if not (regexMatch "^https?://[^/?#]+/?$" $named) -}}
{{- fail (printf "APP_BASE_URL must be an origin, scheme://host[:port] with no path, but server.env, server.baseUrl or commonEnv gives %q" $named) -}}
{{- end -}}
{{- $named | trimSuffix "/" -}}
{{- else if and $host (not (contains "*" $host)) -}}
{{- $wildcard := regexReplaceAll "^[^.]+" $host "*" -}}
{{- $scheme := "http" -}}
{{- range .Values.ingress.tls -}}
{{- if or (not .hosts) (has $host .hosts) (has $wildcard .hosts) -}}
{{- $scheme = "https" -}}
{{- end -}}
{{- end -}}
{{- printf "%s://%s" $scheme $host -}}
{{- end -}}
{{- end }}

{{/*
ZONE_TRUSTED_PROXIES for zone-server: server.env wins over server.trustedProxies, and empty
leaves it unset, so zone-server trusts loopback only.
*/}}
{{- define "zone-apps.server.trustedProxies" -}}
{{- coalesce (get (default dict .Values.server.env) "ZONE_TRUSTED_PROXIES") .Values.server.trustedProxies | default "" | toString | trim -}}
{{- end }}

{{/*
The entries of ZONE_TRUSTED_PROXIES wider than one host, joined with ", ".
A loopback network counts as one host: only the pod itself reaches it.
*/}}
{{- define "zone-apps.server.trustedNetworks" -}}
{{- $networks := list -}}
{{- range splitList "," (include "zone-apps.server.trustedProxies" .) -}}
{{- $entry := trim . -}}
{{- if contains "/" $entry -}}
{{- $address := splitList "/" $entry | first -}}
{{- $prefix := splitList "/" $entry | last | atoi -}}
{{- $host := ternary 128 32 (contains ":" $address) -}}
{{- $loopback := and (hasPrefix "127." $address) (ge $prefix 8) -}}
{{- if not (or (ge $prefix $host) $loopback) -}}
{{- $networks = append $networks $entry -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- join ", " $networks -}}
{{- end }}

{{/*
Secret name used by zone-server
*/}}
{{- define "zone-apps.secretsName" -}}
{{- if and .Values.server.enabled (not .Values.secrets.create) (not .Values.secrets.existingSecret) }}
{{- fail "secrets.existingSecret is required when server.enabled=true and secrets.create=false" }}
{{- else if and (not .Values.secrets.create) .Values.secrets.existingSecret }}
{{- .Values.secrets.existingSecret }}
{{- else }}
{{- printf "%s-secrets" (include "zone-apps.fullname" .) }}
{{- end }}
{{- end }}

{{- define "zone-apps.secretKey" -}}
{{- $keys := default dict .root.Values.secrets.keys -}}
{{- $override := "" -}}
{{- if hasKey $keys .name }}
{{- $override = index $keys .name -}}
{{- end }}
{{- default .default $override -}}
{{- end }}
