# Load test infrastructure for Kubernetes

CHARTS_DIR := ../../charts
HELM_DIR := helm
K8S_DIR := k8s
NAMESPACE := save-test
RELEASE_NAME := save
IMAGE_NAME := save-loadtest
IMAGE_TAG := latest
TEST ?= test_quick_smoke

.PHONY: help check build-image \
        deploy-smoke deploy-medium deploy-large teardown \
        run-loadtest save-results logs logs-follow status port-forward

.DEFAULT_GOAL := help

help:
	@echo "Usage: make <target>"
	@echo ""
	@echo "Deploy:      deploy-smoke, deploy-medium, deploy-large, teardown"
	@echo "Load test:   build-image, run-loadtest TEST=<name>, save-results"
	@echo "Monitor:     status, logs, logs-follow, port-forward"

check:
	@command -v kubectl >/dev/null 2>&1 || { echo "kubectl not found"; exit 1; }
	@command -v helm >/dev/null 2>&1 || { echo "helm not found"; exit 1; }

deploy-smoke: check
	helm upgrade --install $(RELEASE_NAME) $(CHARTS_DIR)/save \
		-f $(HELM_DIR)/values-smoke.yaml \
		-n $(NAMESPACE) --create-namespace \
		--set fullnameOverride=save \
		--wait --timeout 5m

deploy-medium: check
	helm upgrade --install $(RELEASE_NAME) $(CHARTS_DIR)/save \
		-f $(HELM_DIR)/values-medium.yaml \
		-n $(NAMESPACE) --create-namespace \
		--set fullnameOverride=save \
		--wait --timeout 10m

deploy-large: check
	helm upgrade --install $(RELEASE_NAME) $(CHARTS_DIR)/save \
		-f $(HELM_DIR)/values-large.yaml \
		-n $(NAMESPACE) --create-namespace \
		--set fullnameOverride=save \
		--wait --timeout 10m

teardown:
	helm uninstall $(RELEASE_NAME) -n $(NAMESPACE) 2>/dev/null || true
	kubectl delete namespace $(NAMESPACE) --ignore-not-found

build-image:
	cd ../.. && docker build -f tests/infra/Dockerfile.loadtest -t $(IMAGE_NAME):$(IMAGE_TAG) .

run-loadtest:
	kubectl delete job save-loadtest -n $(NAMESPACE) --ignore-not-found
	sed 's/value: "test_quick_smoke"/value: "$(TEST)"/' $(K8S_DIR)/loadtest-job.yaml | kubectl apply -n $(NAMESPACE) -f -
	kubectl wait --for=condition=complete job/save-loadtest -n $(NAMESPACE) --timeout=30m || \
		(kubectl logs job/save-loadtest -n $(NAMESPACE) && exit 1)
	kubectl logs job/save-loadtest -n $(NAMESPACE)

save-results:
	@mkdir -p results
	@TIMESTAMP=$$(date +%Y%m%d-%H%M%S); \
	kubectl logs job/save-loadtest -n $(NAMESPACE) > results/loadtest-$$TIMESTAMP.txt && \
	echo "Results saved to results/loadtest-$$TIMESTAMP.txt"

status:
	@kubectl get namespace $(NAMESPACE) 2>/dev/null || echo "Namespace not found"
	@kubectl get pods -n $(NAMESPACE) -o wide 2>/dev/null || true
	@kubectl get svc -n $(NAMESPACE) 2>/dev/null || true

logs:
	kubectl logs -l app.kubernetes.io/name=save -n $(NAMESPACE) --tail=100

logs-follow:
	kubectl logs -l app.kubernetes.io/name=save -n $(NAMESPACE) -f

port-forward:
	kubectl port-forward svc/save 9000:9000 -n $(NAMESPACE)
