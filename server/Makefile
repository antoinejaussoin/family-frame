.PHONY: serve watch test docker-build docker-run docker

IMAGE := antoinejaussoin/family-frame-server

all:
	cargo run

# Local only: rebuild and restart when source / templates / config change.
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
