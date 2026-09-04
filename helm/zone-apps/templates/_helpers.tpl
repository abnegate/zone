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
{{- range $key, $value := .Values.commonEnv }}
- name: {{ $key }}
  value: {{ $value | quote }}
{{- end }}
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
