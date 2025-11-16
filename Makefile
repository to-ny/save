.PHONY: build test fmt clippy docker-build

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
