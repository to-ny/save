.PHONY: build test integration-test fmt clippy docker-build

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

integration-test:
	cargo test --features compat_tests --features concurrency_tests --features crash_tests

docker-build:
	DOCKER_BUILDKIT=1 docker build \
		--build-arg BUILDKIT_INLINE_CACHE=1 \
		-t $(IMAGE_NAME):$(IMAGE_TAG) \
		.
