# AIO-style dev sandbox - build orchestration.
#
# Build order matters: app/Dockerfile is `FROM sandbox-base`, so the
# `sandbox-base` image must be built and tagged before `app` builds.
# `make up` / `make build` handle this for you.
#
# Profiles: pass space-separated profiles via PROFILES to enable optional
# services (code-server, vnc, ...). E.g. `make up PROFILES=code-server`.
# With no PROFILES, only the always-on services (gateway + app) start - this
# preserves the Phase A-C behavior exactly.

SANDBOX_PASS ?= admin
HASH_FILE := gateway/secrets/hash
COMPOSE := docker compose
PROFILES ?=
PROFILE_FLAGS := $(patsubst %,--profile %,$(PROFILES))

.PHONY: build-base build up down restart logs hash ensure-hash clean

# Build & tag the shared dev-env base image (Dockerfile.base -> sandbox-base).
build-base:
	docker build -t sandbox-base -f Dockerfile.base .

# Build everything: base image first, then compose services (app + any profile
# services whose images are always built).
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

# Build + start the stack (detached). Builds sandbox-base, ensures the auth
# hash exists, then builds and starts app + gateway (+ profile services).
up: build-base ensure-hash
	$(COMPOSE) $(PROFILE_FLAGS) up -d --build

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
	docker rmi sandbox-app sandbox-base sandbox-code-server sandbox-vnc 2>/dev/null || true
