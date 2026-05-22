{{/*
Expand the name of the chart.
*/}}
{{- define "memobuild.name" -}}
{{- if .Values.nameOverride }}{{ .Values.nameOverride }}{{- else }}{{ .Chart.Name }}{{- end }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "memobuild.fullname" -}}
{{- if .Values.fullnameOverride }}{{ .Values.fullnameOverride }}{{- else }}{{ .Release.Name }}-{{ include "memobuild.name" . }}{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "memobuild.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | lower }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "memobuild.labels" -}}
helm.sh/chart: {{ include "memobuild.chart" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/part-of: {{ include "memobuild.name" . }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "memobuild.selectorLabels" -}}
app.kubernetes.io/name: {{ include "memobuild.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "memobuild.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}{{ include "memobuild.fullname" . }}{{- else }}{{ .Values.serviceAccount.name }}{{- end }}
{{- end }}