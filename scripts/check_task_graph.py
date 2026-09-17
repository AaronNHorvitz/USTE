#!/usr/bin/env python3
"""Validate TASKS.md dependency references and distribution-gate invariants."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TASKS = ROOT / "TASKS.md"
ROW = re.compile(r"^\| \[[ x]\] (T-\d+) \|.*?\| ([^|]+) \|", re.MULTILINE)
TASK = re.compile(r"T-\d+")


def fail(message: str) -> None:
    print(f"task graph error: {message}", file=sys.stderr)
    raise SystemExit(1)


text = TASKS.read_text(encoding="utf-8")
dependencies: dict[str, set[str]] = {}
for task_id, dependency_cell in ROW.findall(text):
    if task_id in dependencies:
        fail(f"duplicate task row {task_id}")
    dependencies[task_id] = set(TASK.findall(dependency_cell))

if not dependencies:
    fail("no task rows found")

for task_id, task_dependencies in dependencies.items():
    missing = task_dependencies - dependencies.keys()
    if missing:
        fail(f"{task_id} references missing tasks {sorted(missing)}")

visiting: set[str] = set()
visited: set[str] = set()


def visit(task_id: str) -> None:
    if task_id in visiting:
        fail(f"dependency cycle reaches {task_id}")
    if task_id in visited:
        return
    visiting.add(task_id)
    for dependency in dependencies[task_id]:
        visit(dependency)
    visiting.remove(task_id)
    visited.add(task_id)


for current in dependencies:
    visit(current)


def ancestors(task_id: str) -> set[str]:
    result: set[str] = set()
    pending = list(dependencies[task_id])
    while pending:
        dependency = pending.pop()
        if dependency not in result:
            result.add(dependency)
            pending.extend(dependencies[dependency])
    return result


for required in ("T-06", "T-07", "T-08", "T-44", "T-62"):
    if required not in dependencies:
        fail(f"required gate task {required} is absent")

if "T-62" in ancestors("T-08"):
    fail("T-62 must not block local implementation task T-08")
if "T-62" not in ancestors("T-44"):
    fail("T-44 must transitively require distribution gate T-62")
if "T-06" not in ancestors("T-07"):
    fail("T-07 must retain the development-governance dependency T-06")

print(
    f"task_graph=ok tasks={len(dependencies)} "
    "local_implementation_gate=T-07 distribution_gate=T-62 release_gate=T-44"
)
