.PHONY: all serve fake watch test docker-build docker-run docker ui ui-dev

IMAGE := antoinejaussoin/family-frame-server
VERSION := $(shell tr -d '[:space:]' < ../VERSION)

all: ui
	cargo run

# API only. Pair with `make ui-dev` for Svelte HMR on :5173.
serve:
	cargo run

# Screenshot mode: demo household sources, live public ones. See README.
fake:
	cargo run -- --fake

ui:
	cd ui && npm ci && npm run build

# Family SPA with hot reload. Proxies /api and /dashboard to :8765.
ui-dev:
	cd ui && npm run dev

# Local only: rebuild and restart when source / templates change.
# Does not watch ui/ — use `make ui-dev` for the family SPA.
watch:
	cargo run --features watch -- --watch

test:
	cargo test --features watch

docker-build:
	docker build \
		--build-arg VERSION=$(VERSION) \
		-t $(IMAGE):local \
		-t $(IMAGE):$(VERSION) \
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
