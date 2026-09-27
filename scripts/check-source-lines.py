"""Reject tracked or new Rust source files longer than 600 physical lines."""

import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
LIMIT = 600
paths = subprocess.check_output(
    ["git", "-C", str(ROOT), "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.rs"]
).split(b"\0")
violations = []
checked = 0
for raw in paths:
    if not raw:
        continue
    relative = os.fsdecode(raw)
    file = ROOT / relative
    if not file.is_file():
        continue  # A tracked file removed during a module move is no longer compiled.
    data = file.read_bytes()
    lines = data.count(b"\n") + int(bool(data) and not data.endswith(b"\n"))
    checked += 1
    if lines > LIMIT:
        violations.append((lines, relative))

for lines, relative in sorted(violations, key=lambda item: (-item[0], item[1])):
    print(f"{relative}: {lines} lines (limit {LIMIT})")
print(f"Checked {checked} Rust source files; {len(violations)} exceed {LIMIT} lines.")
sys.exit(bool(violations))
