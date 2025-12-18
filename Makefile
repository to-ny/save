.PHONY: build test fmt clippy docker-build helm-lint helm-template \
        dev-deploy dev-teardown dev-logs dev-port-forward

IMAGE_NAME ?= save
IMAGE_TAG ?= latest

build:
	cargo build --release

test:
	cargo test

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

docker-build:
	DOCKER_BUILDKIT=1 docker build \
		--build-arg BUILDKIT_INLINE_CACHE=1 \
		-t $(IMAGE_NAME):$(IMAGE_TAG) \
		.

helm-lint:
	helm lint charts/save

helm-template:
	helm template save charts/save

dev-deploy:
	cd charts/save-dev && helm dependency update
	helm upgrade --install save-dev ./charts/save-dev -n save-dev --create-namespace --wait

dev-teardown:
	helm uninstall save-dev -n save-dev 2>/dev/null || true
	kubectl delete namespace save-dev --ignore-not-found

dev-logs:
	kubectl logs -l app.kubernetes.io/name=save -n save-dev -f

dev-port-forward:
	kubectl port-forward svc/save-dev-save 9000:9000 -n save-dev
