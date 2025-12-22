# Kubernetes Operator

## Goal
Kubernetes Operator for Save cluster lifecycle management (bootstrap, scaling, healing).

Reference: [ADR-007](../docs/adrs/007-kubernetes-native-deployment.md)

---

## Setup
- [ ] Choose Operator framework (kubebuilder recommended)
- [ ] Initialize Operator project
- [ ] Define SaveCluster CRD schema

---

## CRD
- [ ] SaveCluster spec (replicas, storage, resources, auth)
- [ ] SaveCluster status (phase, conditions, endpoints)
- [ ] CRD validation
- [ ] Generate manifests

---

## Controller - Bootstrap
- [ ] Reconcile loop skeleton
- [ ] Create StatefulSet, Services from spec
- [ ] Wait for pods running
- [ ] Call /cluster/initialize
- [ ] Update status

---

## Controller - Scaling
- [ ] Detect replica count changes
- [ ] Scale up: add node via /cluster/members
- [ ] Scale down: graceful leave before deletion

---

## Controller - Healing
- [ ] Detect failed pods
- [ ] Trigger replacement
- [ ] Rejoin cluster

---

## Controller - Upgrades
- [ ] Rolling update with leadership transfer
- [ ] Health verification between updates

---

## Observability
- [ ] Operator metrics
- [ ] Status conditions (Initialized, Ready, Degraded)
- [ ] Kubernetes events

---

## Testing
- [ ] Unit tests for reconcile logic
- [ ] Integration: cluster creation
- [ ] Integration: scale up/down
- [ ] Integration: pod failure recovery
- [ ] Integration: rolling upgrade
- [ ] E2E on kind

---

## Deployment
- [ ] Operator Helm chart
- [ ] RBAC manifests

---

## Documentation
- [ ] Operator installation guide
- [ ] Cluster deployment documentation (3-node, 5-node setups)
- [ ] Document cluster bootstrap procedure
- [ ] Document cluster upgrade strategy (rolling upgrades)
- [ ] Add troubleshooting guide (partition recovery, replication lag)
- [ ] Create runbook for leader failure scenarios
- [ ] Backup/restore procedures
- [ ] Disaster recovery documentation
