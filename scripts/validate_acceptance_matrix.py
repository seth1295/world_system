#!/usr/bin/env python3
"""Check that the foundation acceptance matrix remains tied to executable evidence."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "conformance" / "acceptance" / "stages-00-03.json"


def reject_duplicate_names(pairs):
    value = {}
    for name, item in pairs:
        if name in value:
            raise ValueError(f"duplicate JSON object name: {name}")
        value[name] = item
    return value


def main() -> int:
    matrix = json.loads(MATRIX.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_names)
    if matrix.get("schema") != "veyra.foundation_acceptance_matrix/1":
        raise SystemExit("unknown foundation acceptance matrix schema")
    stages = matrix.get("stages")
    if not isinstance(stages, list) or [stage.get("stage") for stage in stages] != [0, 1, 2, 3]:
        raise SystemExit("acceptance matrix must cover Stages 0, 1, 2, and 3 exactly once")
    seen_ids = set()
    count = 0
    deferred = 0
    for stage in stages:
        requirements = stage.get("requirements")
        if not isinstance(requirements, list) or not requirements:
            raise SystemExit(f"Stage {stage.get('stage')} has no acceptance evidence")
        for requirement in requirements:
            identifier = requirement.get("id")
            if not isinstance(identifier, str) or identifier in seen_ids:
                raise SystemExit(f"duplicate or invalid acceptance ID: {identifier!r}")
            seen_ids.add(identifier)
            if not requirement.get("requirement"):
                raise SystemExit(f"acceptance item {identifier} has no requirement text")
            status = requirement.get("status", "verified")
            if status not in ("verified", "pass", "deferred"):
                raise SystemExit(f"acceptance item {identifier} has unsupported status {status!r}")
            if status == "deferred":
                deferred += 1
                if not requirement.get("reason"):
                    raise SystemExit(f"deferred acceptance item {identifier} has no reason")
            checks = requirement.get("checks")
            if not isinstance(checks, list) or not checks:
                raise SystemExit(f"acceptance item {identifier} has no machine-checkable evidence")
            if status == "pass" and not any(isinstance(check.get("run"), list) for check in checks):
                raise SystemExit(f"passed acceptance item {identifier} must execute a regression check")
            for check in checks:
                run = check.get("run")
                relative = check.get("path")
                contains = check.get("contains")
                absent = check.get("absent")
                evidence_kinds = sum((isinstance(run, list), isinstance(contains, str), isinstance(absent, str)))
                if evidence_kinds != 1:
                    raise SystemExit(f"acceptance item {identifier} has a malformed evidence check")
                if isinstance(run, list):
                    if not run or not all(isinstance(part, str) and part for part in run):
                        raise SystemExit(f"acceptance item {identifier} has an invalid command check")
                    result = subprocess.run(run, cwd=ROOT, text=True, capture_output=True, check=False)
                    if result.returncode != 0:
                        raise SystemExit(
                            f"acceptance item {identifier} command failed ({result.returncode}): "
                            f"{' '.join(run)}\n{result.stdout}{result.stderr}"
                        )
                    if result.stdout:
                        print(result.stdout.rstrip())
                    continue
                if not isinstance(relative, str):
                    raise SystemExit(f"acceptance item {identifier} has a malformed source check")
                path = (ROOT / relative).resolve()
                if not path.is_relative_to(ROOT.resolve()) or not path.is_file():
                    raise SystemExit(f"acceptance item {identifier} references missing or out-of-root file {relative}")
                text = path.read_text(encoding="utf-8")
                if isinstance(contains, str) and contains not in text:
                    raise SystemExit(f"acceptance item {identifier} no longer matches {relative}: {contains!r}")
                if isinstance(absent, str) and absent in text:
                    raise SystemExit(f"acceptance item {identifier} deferred boundary changed in {relative}: {absent!r}")
            count += 1
    print(f"PASS: {count - deferred} Stage 0-3 requirements passed source or executable checks; {deferred} deferred with reasons")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
