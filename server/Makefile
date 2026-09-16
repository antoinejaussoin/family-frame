.PHONY: serve watch test docker-build docker-run docker ui

IMAGE := antoinejaussoin/family-frame-server

all: ui
	cargo run

ui:
	cd ui && npm ci && npm run build

# Local only: rebuild and restart when source / templates change.
watch:
	cargo run -- --watch

test:
	cargo test

docker-build:
	docker build \
		-t $(IMAGE):local \
		-t $(IMAGE):latest \
		.

DOCKER_RUN_MOUNTS :=
ifneq ($(wildcard config.toml),)
DOCKER_RUN_MOUNTS += -v $(CURDIR)/config.toml:/app/config.toml:ro
endif

docker-run:
	docker run --rm --init --name family-frame-server --shm-size=1g -p 8765:8765 \
		$(DOCKER_RUN_MOUNTS) \
		$(IMAGE):local

docker: docker-build docker-run
