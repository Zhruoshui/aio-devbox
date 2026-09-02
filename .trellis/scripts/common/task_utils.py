#!/usr/bin/env python3
"""
Task directory resolution and lifecycle hooks.

Provides:
    resolve_task_dir      - Resolve a task input (name / relative path / absolute path) to its directory
    find_task_by_name     - Find a task directory by dir name inside the tasks tree
    archive_task_complete - Move a task directory into the archive
    run_task_hooks        - Run lifecycle hooks bound to an event in task.json
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

from .log import Colors, colored
from .paths import DIR_WORKFLOW, DIR_TASKS, DIR_ARCHIVE, FILE_TASK_JSON, get_tasks_dir


# =============================================================================
# Task Directory Resolution
# =============================================================================

def resolve_task_dir(task_input: str, repo_root: Path) -> Path:
    """Resolve a task input to its task directory.

    Supports:
        - Task name (e.g., 'my-task'), matched against active tasks and
          date-prefixed directories (e.g., '01-31-my-task')
        - Relative path from repo root (e.g., '.trellis/tasks/01-31-my-task')
        - Absolute path

    Args:
        task_input: Task name or path as provided by the user.
        repo_root: Repository root path.

    Returns:
        Resolved task directory path (not guaranteed to exist).
    """
    tasks_dir = get_tasks_dir(repo_root)
    candidate = Path(task_input)

    # Absolute path — use as-is
    if candidate.is_absolute():
        return candidate

    # Relative path from repo root (e.g. .trellis/tasks/01-31-my-task)
    if task_input.startswith(f"{DIR_WORKFLOW}/") or task_input.startswith(f"./{DIR_WORKFLOW}/"):
        return (repo_root / task_input).resolve()

    # Exact directory name inside tasks dir
    exact = tasks_dir / task_input
    if exact.is_dir():
        return exact

    # Bare task name — match date-prefixed dirs ending with '-<name>'
    # (also covers archived tasks so finish/archive can resolve them)
    matches: list[Path] = []
    for base in (tasks_dir, tasks_dir / DIR_ARCHIVE):
        if not base.is_dir():
            continue
        for d in base.iterdir():
            if d.is_dir() and (d.name == task_input or d.name.endswith(f"-{task_input}")):
                matches.append(d)

    if len(matches) == 1:
        return matches[0]

    # Fall back to the naive location so callers can print a useful error
    return exact


# =============================================================================
# Task Lookup / Archive
# =============================================================================

def find_task_by_name(dir_name: str, tasks_dir: Path) -> Path | None:
    """Find a task directory by its directory name.

    Searches active tasks first, then the archive.

    Args:
        dir_name: Task directory name (e.g., '01-31-my-task').
        tasks_dir: Path to the tasks directory.

    Returns:
        Path to the task directory, or None if not found.
    """
    for base in (tasks_dir, tasks_dir / DIR_ARCHIVE):
        if not base.is_dir():
            continue
        candidate = base / dir_name
        if candidate.is_dir():
            return candidate
        # Also match date-prefixed dirs for a bare slug
        for d in sorted(base.iterdir()):
            if d.is_dir() and (d.name == dir_name or d.name.endswith(f"-{dir_name}")):
                return d
    return None


def archive_task_complete(task_dir: Path, repo_root: Path) -> dict:
    """Move a completed task directory into the archive.

    The destination is ``.trellis/tasks/archive/<YYYY-MM>/<dir_name>``.

    Args:
        task_dir: Path to the task directory to archive.
        repo_root: Repository root path.

    Returns:
        ``{"archived_to": "<dest path>"}`` on success, ``{}`` on failure.
    """
    try:
        tasks_dir = get_tasks_dir(repo_root)
        archive_dir = tasks_dir / DIR_ARCHIVE
        month = datetime.now().strftime("%Y-%m")
        dest_dir = archive_dir / month / task_dir.name
        dest_dir.parent.mkdir(parents=True, exist_ok=True)

        if dest_dir.exists():
            print(
                colored(f"Error: Archive destination already exists: {dest_dir}", Colors.RED),
                file=sys.stderr,
            )
            return {}

        shutil.move(str(task_dir), str(dest_dir))
        return {"archived_to": str(dest_dir)}
    except Exception as exc:
        print(
            colored(f"Error: Failed to archive task: {exc}", Colors.RED),
            file=sys.stderr,
        )
        return {}


# =============================================================================
# Lifecycle Hooks
# =============================================================================

def run_task_hooks(event: str, task_json_path: Path, repo_root: Path) -> None:
    """Run lifecycle hooks bound to an event in task.json.

    Each hook command receives the TASK_JSON_PATH environment variable
    pointing to task.json. Hook failures print a warning but do not block
    the main operation.

    Args:
        event: Hook event name (after_create / after_start / after_finish / after_archive).
        task_json_path: Path to the task's task.json.
        repo_root: Repository root path (hooks run with this as cwd).
    """
    if not task_json_path.is_file():
        return

    try:
        import json

        data = json.loads(task_json_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return

    hooks = data.get("hooks") or {}
    commands = hooks.get(event) or []
    if not commands:
        return

    env = os.environ.copy()
    env["TASK_JSON_PATH"] = str(task_json_path)

    for command in commands:
        if not isinstance(command, str) or not command.strip():
            continue
        try:
            result = subprocess.run(
                command,
                shell=True,
                cwd=repo_root,
                env=env,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if result.returncode != 0:
                detail = (result.stderr or result.stdout or "").strip()
                print(
                    colored(
                        f"Warning: hook '{event}' failed (exit {result.returncode}): {command}",
                        Colors.YELLOW,
                    ),
                    file=__import__("sys").stderr,
                )
                if detail:
                    print(detail, file=sys.stderr)
        except Exception as exc:  # never block the main operation
            print(
                colored(f"Warning: hook '{event}' raised: {exc}", Colors.YELLOW),
                file=sys.stderr,
            )
