.PHONY: serve watch test pico-sim docker-build docker-run docker

IMAGE := antoinejaussoin/family-frame-server

serve:
	cargo run -- serve

# Local only: rebuild and restart when source / templates / config change.
watch:
	cargo run -- serve --watch

test:
	cargo test

pico-sim:
	cargo run -- pico-sim --url http://127.0.0.1:8765 --interval-secs 5

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
