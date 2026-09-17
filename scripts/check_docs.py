#!/usr/bin/env python3
"""Check active Markdown links and requirement/task/test references."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PREFIX = r"FR|NFR|T|VT|BM|D"
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
REFERENCE = re.compile(
    rf"\b(?P<prefix>{PREFIX})-(?P<number>\d+)"
    rf"(?P<tail>(?:(?:/|…|–)(?:(?:{PREFIX})-)?\d+)*)"
)
SUFFIX = re.compile(rf"(?:/|…|–)(?:(?P<prefix>{PREFIX})-)?(?P<number>\d+)")
DEFINITION_PATTERNS = {
    "FR": (ROOT / "PRD.md", re.compile(r"^\| (FR-\d{2}) \|", re.MULTILINE)),
    "NFR": (ROOT / "PRD.md", re.compile(r"^\| (NFR-\d{2}) \|", re.MULTILINE)),
    "T": (ROOT / "TASKS.md", re.compile(r"^\| \[[ x]\] (T-\d{2}) \|", re.MULTILINE)),
    "VT": (
        ROOT / "docs/verification-and-benchmarks.md",
        re.compile(r"^\| (VT-\d{2}) \|", re.MULTILINE),
    ),
    "BM": (
        ROOT / "docs/verification-and-benchmarks.md",
        re.compile(r"^\| (BM-\d{2}) \|", re.MULTILINE),
    ),
    "D": (ROOT / "architecture.md", re.compile(r"^\| (D-\d{2}) \|", re.MULTILINE)),
}
REQUIRED_DOCUMENTS = (
    "README.md",
    "PRD.md",
    "TASKS.md",
    "architecture.md",
    "SECURITY.md",
    "PROGRESS.md",
    "docs/implementation-plan.md",
    "docs/application-use-cases.md",
)


def markdown_files() -> list[Path]:
    return sorted(path for path in ROOT.rglob("*.md") if ".git" not in path.parts)


def active_markdown() -> list[Path]:
    return [path for path in markdown_files() if "history" not in path.parts]


def referenced_identifiers(text: str) -> list[str]:
    identifiers: list[str] = []
    for match in REFERENCE.finditer(text):
        inherited_prefix = match.group("prefix")
        identifiers.append(f"{inherited_prefix}-{match.group('number')}")
        for suffix in SUFFIX.finditer(match.group("tail")):
            prefix = suffix.group("prefix") or inherited_prefix
            identifiers.append(f"{prefix}-{suffix.group('number')}")
    return identifiers


errors: list[str] = []
for relative in REQUIRED_DOCUMENTS:
    if not (ROOT / relative).is_file():
        errors.append(f"missing required document: {relative}")

definitions: dict[str, set[str]] = {}
for prefix, (source, pattern) in DEFINITION_PATTERNS.items():
    definitions[prefix] = set(pattern.findall(source.read_text(encoding="utf-8")))

for path in markdown_files():
    text = path.read_text(encoding="utf-8")
    display = path.relative_to(ROOT)
    for target in LINK.findall(text):
        if target.startswith(("https://", "http://", "mailto:", "#")):
            continue
        file_target = target.split("#", 1)[0]
        if file_target:
            resolved = (path.parent / file_target).resolve()
            if ROOT != resolved and ROOT not in resolved.parents:
                errors.append(f"{display}: local link escapes repository: {target}")
            elif not resolved.exists():
                errors.append(f"{display}: broken local link: {target}")

for path in active_markdown():
    text = path.read_text(encoding="utf-8")
    display = path.relative_to(ROOT)
    for identifier in referenced_identifiers(text):
        prefix = identifier.split("-", 1)[0]
        if identifier not in definitions[prefix]:
            errors.append(f"{display}: undefined reference: {identifier}")

if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)

print(
    f"documentation=ok links={len(markdown_files())} active_ids={len(active_markdown())} "
    f"definitions={sum(len(values) for values in definitions.values())}"
)
