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
# Accept BOTH "code-server vnc" (space-separated) and "code-server,vnc"
# (comma-separated, the form README/docs use). docker compose --profile does
# NOT split commas - feeding it `--profile code-server,vnc` silently matches
# zero services (observed: down/up skipped code-server + vnc entirely) - so
# normalize commas to spaces first, then one --profile flag per word.
comma := ,
PROFILE_FLAGS := $(addprefix --profile ,$(subst $(comma), ,$(PROFILES)))

# aio-config: the scenario configurator image (build/host-side tool).
AIO_CONFIG_IMAGE := aio-config
# Run the configurator as the host uid so the files it writes (.aio/enabled.toml,
# Dockerfile.base) are host-owned, not root-owned.
UID := $(shell id -u)
GID := $(shell id -g)

.PHONY: build-base build build-config config gen up down restart logs hash ensure-hash save load clean

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

# --- Offline whole-stack transfer -------------------------------------------
# `docker save`/`load` only carries IMAGES. The stack additionally needs two
# gitignored HOST files to start: .env (compose env_file - a hard requirement;
# compose refuses to up without it) and gateway/secrets/hash (basicauth bcrypt;
# auto-regenerated with the DEFAULT password if missing). `make save` bundles
# the runtime image set (docker save dedupes shared layers across images in one
# tar, so including sandbox-base adds ~nothing beyond app/code-server) plus
# those files + the scenario selection; `make load` restores everything on the
# offline machine. Then start with: make up NOBUILD=1 PROFILES="code-server vnc"
#
# NOT included (by design): the workspace volume aio_workspace (user data -
# pi sessions/auth, code-server settings, chromium profile). Migrate user data
# separately (docs/offline-install-guide.md §3.4); a fresh volume starts empty.
OFFLINE_BUNDLE ?= aio-offline-bundle
SAVE_IMAGES ?= sandbox-base sandbox-app sandbox-code-server sandbox-vnc caddy:2

# Bundle images + host-only state for transfer to an offline machine.
# Produces a DIRECTORY (not a double tar - a tar-of-the-tar would need 2x the
# disk at peak); transfer it whole (tar cf bundle.tar aio-offline-bundle / scp -r).
save:
	@test -f .env || { echo "save: .env missing (compose requires it; see .env.example)" >&2; exit 1; }
	@test -f $(HASH_FILE) || { echo "save: $(HASH_FILE) missing (run: make hash)" >&2; exit 1; }
	rm -rf $(OFFLINE_BUNDLE)
	mkdir -p $(OFFLINE_BUNDLE)
	docker save $(SAVE_IMAGES) -o $(OFFLINE_BUNDLE)/images.tar
	cp .env $(OFFLINE_BUNDLE)/env
	cp $(HASH_FILE) $(OFFLINE_BUNDLE)/hash
	cp .aio/enabled.toml $(OFFLINE_BUNDLE)/enabled.toml
	@du -sh $(OFFLINE_BUNDLE)
	@echo "wrote $(OFFLINE_BUNDLE)/: images.tar ($(SAVE_IMAGES)) + env + hash + enabled.toml"

# Restore a bundle produced by `make save` (images + .env + gateway hash +
# scenario selection) on the offline machine.
load:
	@test -f $(OFFLINE_BUNDLE)/images.tar || { echo "load: $(OFFLINE_BUNDLE)/images.tar not found (produce it with: make save)" >&2; exit 1; }
	docker load -i $(OFFLINE_BUNDLE)/images.tar
	cp $(OFFLINE_BUNDLE)/env .env
	mkdir -p gateway/secrets && cp $(OFFLINE_BUNDLE)/hash $(HASH_FILE)
	mkdir -p .aio && cp $(OFFLINE_BUNDLE)/enabled.toml .aio/enabled.toml
	@echo "restored images + .env + gateway hash + scenario selection. Start with: make up NOBUILD=1 PROFILES=\"code-server vnc\" (then run aio-pi-extensions once in a terminal)"
