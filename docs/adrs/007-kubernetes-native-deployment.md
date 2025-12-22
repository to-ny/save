# ADR-007: Kubernetes-Native Deployment Strategy

**Status**: Accepted

## Context

Save requires a deployment and orchestration strategy for production environments. As a distributed system with Raft consensus, cluster formation, scaling, and healing require careful coordination.

Key requirements:
- Automated cluster bootstrap (no manual initialization steps)
- Dynamic scaling (add/remove nodes safely)
- Self-healing (recover from node failures)
- Simple operational experience

## Decision

Adopt **Kubernetes as the primary deployment target** with a dedicated Kubernetes Operator for cluster lifecycle management.

**Architecture**:
- Kubernetes Operator manages SaveCluster custom resources
- Operator handles bootstrap, scaling, healing, upgrades
- Core storage engine remains platform-agnostic internally
- Helm chart for initial deployment, Operator for day-2 operations

**Discovery & Bootstrap**:
- DNS-based peer discovery via headless Service
- Operator-driven cluster initialization
- StatefulSet with ordinal-based node IDs

## Alternatives Considered

**Platform-Agnostic (MinIO model)**:
- Pros: Run anywhere (bare metal, VMs, Docker, K8s), larger addressable market
- Cons: Must implement discovery, bootstrap, healing in application; direct competition with mature incumbent
- Rejected: Significant effort to match MinIO's maturity; less differentiation

**Self-Bootstrap without Operator**:
- Pros: No operator dependency, simpler deployment
- Cons: Complex distributed coordination in application code, edge cases (split-brain, races)
- Rejected: Application should focus on storage, not orchestration

**External Coordination (etcd/Consul for bootstrap)**:
- Pros: Battle-tested coordination primitives
- Cons: External dependency, contradicts ADR-006's self-contained principle
- Rejected: Architectural mismatch

## Consequences

**Positive**:
- Clean separation: storage engine vs. deployment orchestration
- Leverage K8s primitives (Services, PVCs, DNS, health probes)
- Integration with cloud-native ecosystem (Prometheus, cert-manager, GitOps)
- Clear market positioning as the Kubernetes-native object store
- Simpler cluster formation (Operator controls sequence explicitly)

**Negative**:
- Requires Kubernetes (or compatible runtime like k3s)
- Excludes bare-metal and VM-only environments initially
- Operator development and maintenance overhead
- Testing requires Kubernetes environment (kind, minikube)

## Future Flexibility

The core storage engine remains platform-agnostic internally. Future phases may introduce alternative deployment modes based on community demand:

- Phase 5+: Docker Compose, systemd units for non-K8s environments
- Pluggable discovery backends (static config, cloud APIs)

This decision prioritizes Kubernetes excellence now while preserving optionality for broader platform support.

## Related

- [ADR-006](006-distributed-metadata-strategy.md): Embedded Raft for self-contained metadata
