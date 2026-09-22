#!/bin/sh
# Seed the mise data directory on the shared workspace volume.
#
# WHY THIS EXISTS
#
# Baked toolchains live in the IMAGE at /opt/mise, because the workspace volume
# mounts over /root and would otherwise mask mise's default ~/.local/share/mise.
# That layout is right for baked tools, but it means a user's runtime
# `mise use -g <tool>` lands on the container WRITABLE LAYER and is lost on
# recreate.
#
# This script makes the volume usable as the data dir TOO, without copying the
# baked toolchains: it symlinks the image's trees into the volume, so the volume
# costs only the links (~KB) plus whatever the user installs. Verified
# 2026-09-22: a seeded volume measured 28KB, and baked rust/go/uv still ran.
#
# DIVISION OF LABOUR
#
#   this script (once, at app-container boot)     -> SEED the volume
#   /etc/profile.d/mise.sh (per login shell)      -> PROBE and pick data dir
#
# profile.d does the probing because it is baked into sandbox-base and shared
# by every derived image: one seeding (by the always-started app container)
# makes the volume live for code-server's and vnc's shells as well. Rewriting
# profile.d at runtime would only fix the container doing the writing.
#
# IDEMPOTENT: safe on every boot. Symlinks are re-pointed with `ln -sfn`, and
# anything the USER installed is never overwritten.
#
# NO-OP when the engine isn't baked (e.g. `docker run --rm <base> bash`):
# the baked-only layout keeps working.

set -eu

MISE_IMAGE_DIR=/opt/mise
# The workspace volume is mounted at $HOME (/root) by docker-compose.yml.
VOL="${HOME:-/root}/.local/share/mise"

[ -d "$MISE_IMAGE_DIR" ] || exit 0

# Require the workspace to actually be a MOUNT POINT. Without this, a bare
# `docker run <base> bash` would seed /root/.local/share/mise into the
# container's writable layer — useless (thrown away with the container) and it
# would also shadow the baked layout with a half-populated copy. `mountpoint`
# is in util-linux, present in the base image; fall back to a /proc check if
# it's ever missing.
is_mount() {
	if command -v mountpoint >/dev/null 2>&1; then
		mountpoint -q "$1"
	else
		grep -q " $1 " /proc/mounts
	fi
}
is_mount "${HOME:-/root}" || exit 0

mkdir -p "$VOL/installs" "$VOL/rustup" "$VOL/cargo" "$VOL/shims" 2>/dev/null || exit 0

link_tree() {
	# link_tree <image-subdir> <volume-subdir>
	# Symlink each entry of the image dir into the volume dir. Never overwrite
	# an entry the user created (a real dir/file); only re-point our own links.
	src="$1"
	dst="$2"
	[ -d "$src" ] || return 0
	for entry in "$src"/*; do
		[ -e "$entry" ] || continue # no match -> literal glob
		name=${entry##*/}
		if [ -e "$dst/$name" ] && [ ! -L "$dst/$name" ]; then
			continue # user-owned; hands off
		fi
		ln -sfn "$entry" "$dst/$name"
	done
}

link_tree "$MISE_IMAGE_DIR/installs" "$VOL/installs"
# rustup/ and cargo/ are NOT optional. `installs/rust/<ver>` is only a symlink
# into RUSTUP_HOME, so without these two the rust shims fail with
# "No version is set for shim: rustc" (observed 2026-09-22).
link_tree "$MISE_IMAGE_DIR/rustup" "$VOL/rustup"
link_tree "$MISE_IMAGE_DIR/cargo" "$VOL/cargo"

# Regenerate the volume's config.toml: the IMAGE's [tools] are authoritative
# (so a rebuilt base with new or re-versioned tools is picked up), and any
# entry the user added is re-appended.
#
# Why regenerate rather than copy-once: MISE_GLOBAL_CONFIG_FILE REPLACES
# MISE_CONFIG_DIR/config.toml instead of layering on top of it (verified —
# `mise config ls` lists exactly one file when both are set). So there is only
# ONE config the shims consult, and it has to carry baked + user tools at once.
# A copy-once volume config would silently lose tools added to the base later.
#
# Why the image file is safe to append to: the engine fragment writes
# `[settings] ... [tools]` and every tool fragment appends `key = value` lines,
# so the image config always ENDS inside its [tools] table.
seed_config() {
	img="$MISE_IMAGE_DIR/config.toml"
	vol="$VOL/config.toml"
	[ -f "$img" ] || return 0

	if [ ! -f "$vol" ]; then
		cp "$img" "$vol"
		return 0
	fi

	tmp="$VOL/.config.toml.tmp"
	cp "$img" "$tmp"
	awk '
		FNR == NR {                       # image config: baked tool names
			if ($0 ~ /^\[tools\]/) { s = 1; next }
			if ($0 ~ /^\[/)        { s = 0 }
			if (s && $0 ~ /=/) { k = $0; sub(/[ \t]*=.*/, "", k); baked[k] = 1 }
			next
		}
		$0 ~ /^\[tools\]/ { s = 1; next }  # volume config: user-only entries
		$0 ~ /^\[/        { s = 0 }
		s && $0 ~ /=/ {
			k = $0; sub(/[ \t]*=.*/, "", k)
			if (!(k in baked)) print
		}
	' "$img" "$vol" >> "$tmp"
	mv "$tmp" "$vol"
}
seed_config

# Generate this data dir's own shim farm. NOT satisfiable by symlinking the
# image's shims dir: mise validates that a shim lives in the data dir it
# belongs to, so a symlinked shims/ makes every tool report
# "rustc is not a valid shim" (observed 2026-09-22). A real reshim writes ~34
# tiny files and makes the volume self-sufficient — including for tools the
# user installs later, which mise re-shims here automatically.
#
# Non-fatal: on failure the volume shims stay incomplete, but the baked shims
# remain on PATH (profile.d lists both dirs), so baked tools keep working.
MISE_DATA_DIR="$VOL" MISE_CONFIG_DIR="$VOL" \
	RUSTUP_HOME="$VOL/rustup" CARGO_HOME="$VOL/cargo" \
	mise reshim >/dev/null 2>&1 || true

echo "mise volume seeded: $VOL (baked tools linked from $MISE_IMAGE_DIR)"
