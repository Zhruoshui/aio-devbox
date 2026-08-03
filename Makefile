# AIO-style dev sandbox - build orchestration.
#
# Build order matters: app/Dockerfile is `FROM sandbox-base`, so the
# `sandbox-base` image must be built and tagged before `app` builds.
# `make up` / `make build` handle this for you.
#
# Scenario presets: `make config` (TUI) writes `.aio/enabled.toml`; `make
# build-base` runs `aio-config gen` to assemble `Dockerfile.base` from
# `Dockerfile.base.head` + enabled `scenarios/<id>/fragment.Dockerfile` +
# `Dockerfile.base.tail`, then builds. See
# .trellis/tasks/08-03-scenario-preset-profiles/{prd,design}.md.
#
# Profiles: pass space-separated profiles via PROFILES to enable optional
# services (code-server, vnc, ...). E.g. `make up PROFILES=code-server`.
# With no PROFILES, only the always-on services (gateway + app) start - this
# preserves the Phase A-C behavior exactly.
#
# NOBUILD=1: skip `build-base` (and thus gen) + `compose --build` - for offline
# machines that `docker load` pre-built images instead of building. E.g.
# `make up NOBUILD=1`.

SANDBOX_PASS ?= admin
HASH_FILE := gateway/secrets/hash
COMPOSE := docker compose
PROFILES ?=
PROFILE_FLAGS := $(patsubst %,--profile %,$(PROFILES))

# aio-config: the scenario configurator image (build/host-side tool).
AIO_CONFIG_IMAGE := aio-config
# Run the configurator as the host uid so the files it writes (.aio/enabled.toml,
# Dockerfile.base) are host-owned, not root-owned.
UID := $(shell id -u)
GID := $(shell id -g)

.PHONY: build-base build build-config config gen up down restart logs hash ensure-hash clean

# Build & tag the aio-config configurator image (online: fetches crates).
build-config:
	docker build -t $(AIO_CONFIG_IMAGE) -f config/Dockerfile config/

# Interactive scenario picker -> writes .aio/enabled.toml.
config: build-config
	docker run --rm -it --user $(UID):$(GID) \
	  -v $(PWD):/repo \
	  $(AIO_CONFIG_IMAGE) tui --repo /repo

# Assemble Dockerfile.base from head + enabled scenario fragments + tail.
# Internal target, run automatically by build-base.
gen: build-config
	docker run --rm --user $(UID):$(GID) \
	  -v $(PWD):/repo \
	  $(AIO_CONFIG_IMAGE) gen --repo /repo

# Build & tag the shared dev-env base image. Runs `gen` first so Dockerfile.base
# reflects the current scenario selection, then `docker build`.
build-base: gen
	docker build -t sandbox-base -f Dockerfile.base .

# Build everything: base image first (with gen), then compose services (app +
# any profile services whose images are always built).
build: build-base
	$(COMPOSE) $(PROFILE_FLAGS) build

# Generate/overwrite the bcrypt hash file for gateway basic-auth.
# Customize the password with PASS=... (default: admin).
hash:
	@mkdir -p gateway/secrets
	docker run --rm caddy:2 caddy hash-password --plaintext "$(SANDBOX_PASS)" > $(HASH_FILE)
	@echo "wrote $(HASH_FILE) (password: $(SANDBOX_PASS))"

# Internal: ensure the hash file exists before the gateway mounts it.
ensure-hash:
	@if [ ! -f $(HASH_FILE) ]; then $(MAKE) hash; fi

# Build + start the stack (detached). Default: build-base (gen + docker build)
# -> compose up --build. NOBUILD=1: skip building entirely (offline: images
# already `docker load`ed).
ifdef NOBUILD
up: ensure-hash
	$(COMPOSE) $(PROFILE_FLAGS) up -d
else
up: build-base ensure-hash
	$(COMPOSE) $(PROFILE_FLAGS) up -d --build
endif

# Stop the stack (keeps images and the workspace volume). Pass the same PROFILES
# used at `up` so profile services are torn down too; without PROFILES only the
# always-on services are affected.
down:
	$(COMPOSE) $(PROFILE_FLAGS) down

restart:
	$(COMPOSE) $(PROFILE_FLAGS) restart

logs:
	$(COMPOSE) $(PROFILE_FLAGS) logs -f

# Destructive: stop, remove the workspace volume, and drop built images.
clean:
	$(COMPOSE) $(PROFILE_FLAGS) down -v
	docker rmi sandbox-app sandbox-base sandbox-code-server sandbox-vnc $(AIO_CONFIG_IMAGE) 2>/dev/null || true
