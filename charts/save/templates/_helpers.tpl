{{/*
Expand the name of the chart.
*/}}
{{- define "save.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "save.fullname" -}}
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
{{- define "save.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "save.labels" -}}
helm.sh/chart: {{ include "save.chart" . }}
{{ include "save.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "save.selectorLabels" -}}
app.kubernetes.io/name: {{ include "save.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "save.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "save.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Create the name of the headless service
*/}}
{{- define "save.headlessServiceName" -}}
{{- printf "%s-headless" (include "save.fullname" .) }}
{{- end }}

{{/*
Create the name of the credentials secret
*/}}
{{- define "save.secretName" -}}
{{- if .Values.auth.existingSecret }}
{{- .Values.auth.existingSecret }}
{{- else }}
{{- printf "%s-credentials" (include "save.fullname" .) }}
{{- end }}
{{- end }}

{{/*
Create the name of the TLS secret
*/}}
{{- define "save.tlsSecretName" -}}
{{- if .Values.tls.existingSecret }}
{{- .Values.tls.existingSecret }}
{{- else }}
{{- printf "%s-tls" (include "save.fullname" .) }}
{{- end }}
{{- end }}

{{/*
Create the name of the configmap
*/}}
{{- define "save.configMapName" -}}
{{- printf "%s-config" (include "save.fullname" .) }}
{{- end }}

{{/*
Return the proper image name
*/}}
{{- define "save.image" -}}
{{- $tag := default .Chart.AppVersion .Values.image.tag }}
{{- printf "%s:%s" .Values.image.repository $tag }}
{{- end }}

{{/*
Return the init job image
*/}}
{{- define "save.initImage" -}}
{{- printf "%s:%s" .Values.clusterInit.image.repository .Values.clusterInit.image.tag }}
{{- end }}

{{/*
Generate peer list for Raft configuration.
Format: "node_id:hostname:port"
*/}}
{{- define "save.peerList" -}}
{{- $fullname := include "save.fullname" . -}}
{{- $headless := include "save.headlessServiceName" . -}}
{{- $namespace := .Release.Namespace -}}
{{- $replicas := int .Values.replicaCount -}}
{{- $peers := list -}}
{{- range $i := until $replicas -}}
{{- $nodeId := add1 $i -}}
{{- $peer := printf "%d:%s-%d.%s.%s.svc.cluster.local:9001" $nodeId $fullname $i $headless $namespace -}}
{{- $peers = append $peers $peer -}}
{{- end -}}
{{- $peers | toJson -}}
{{- end }}

{{/*
Generate peer list for a specific node (excluding self).
Takes a dict with "root" (context) and "nodeIndex" (0-based index).
*/}}
{{- define "save.peerListForNode" -}}
{{- $root := .root -}}
{{- $nodeIndex := .nodeIndex -}}
{{- $fullname := include "save.fullname" $root -}}
{{- $headless := include "save.headlessServiceName" $root -}}
{{- $namespace := $root.Release.Namespace -}}
{{- $replicas := int $root.Values.replicaCount -}}
{{- $peers := list -}}
{{- range $i := until $replicas -}}
{{- if ne $i $nodeIndex -}}
{{- $nodeId := add1 $i -}}
{{- $peer := printf "%d:%s-%d.%s.%s.svc.cluster.local:9001" $nodeId $fullname $i $headless $namespace -}}
{{- $peers = append $peers $peer -}}
{{- end -}}
{{- end -}}
{{- $peers | toJson -}}
{{- end }}

{{/*
Generate the cluster members list for initialization.
Format: ["1:host1:9001", "2:host2:9001", ...]
*/}}
{{- define "save.clusterMembers" -}}
{{- $fullname := include "save.fullname" . -}}
{{- $headless := include "save.headlessServiceName" . -}}
{{- $namespace := .Release.Namespace -}}
{{- $replicas := int .Values.replicaCount -}}
{{- $members := list -}}
{{- range $i := until $replicas -}}
{{- $nodeId := add1 $i -}}
{{- $member := printf "%d:%s-%d.%s.%s.svc.cluster.local:9001" $nodeId $fullname $i $headless $namespace -}}
{{- $members = append $members $member -}}
{{- end -}}
{{- $members | toJson -}}
{{- end }}

{{/*
Return the FQDN for a pod by ordinal index
*/}}
{{- define "save.podFQDN" -}}
{{- $fullname := include "save.fullname" .root -}}
{{- $headless := include "save.headlessServiceName" .root -}}
{{- $namespace := .root.Release.Namespace -}}
{{- printf "%s-%d.%s.%s.svc.cluster.local" $fullname .index $headless $namespace -}}
{{- end }}

{{/*
Default pod anti-affinity
*/}}
{{- define "save.defaultAntiAffinity" -}}
podAntiAffinity:
  preferredDuringSchedulingIgnoredDuringExecution:
    - weight: 100
      podAffinityTerm:
        labelSelector:
          matchLabels:
            {{- include "save.selectorLabels" . | nindent 12 }}
        topologyKey: kubernetes.io/hostname
{{- end }}

{{/*
Merge user-provided affinity with default anti-affinity
*/}}
{{- define "save.affinity" -}}
{{- if .Values.affinity }}
{{- toYaml .Values.affinity }}
{{- else }}
{{- include "save.defaultAntiAffinity" . }}
{{- end }}
{{- end }}

{{/*
Validate replicaCount is odd for Raft quorum
*/}}
{{- define "save.validateReplicaCount" -}}
{{- $count := int .Values.replicaCount -}}
{{- if and (gt $count 1) (eq (mod $count 2) 0) -}}
{{- fail "replicaCount must be odd (1, 3, 5, 7, ...) for Raft quorum" -}}
{{- end -}}
{{- end }}
