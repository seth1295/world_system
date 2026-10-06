#!/usr/bin/env python3
"""Independently derive the direction-cube face-edge adjacency table."""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / "schema" / "face_adjacency.toml"
EDGES = (("U-", "U", -1), ("U+", "U", 1), ("V-", "V", -1), ("V+", "V", 1))


def face_point(face: int, u: float, v: float) -> tuple[float, float, float]:
    return (
        (1.0, u, v),
        (-u, 1.0, v),
        (-u, -v, 1.0),
        (-1.0, -v, -u),
        (v, -1.0, -u),
        (v, u, -1.0),
    )[face]


def point_to_face(x: float, y: float, z: float) -> tuple[int, float, float]:
    components = (x, y, z)
    axis = max(range(3), key=lambda index: (abs(components[index]), -index))
    face_for_axis = ((0, 3), (1, 4), (2, 5))
    face = face_for_axis[axis][0 if components[axis] > 0 else 1]
    if face == 0:
        return face, y / x, z / x
    if face == 1:
        return face, -x / y, z / y
    if face == 2:
        return face, -x / z, -y / z
    if face == 3:
        return face, z / x, y / x
    if face == 4:
        return face, z / y, -x / y
    return face, -y / z, -x / z


def derive_one(face: int, edge_name: str, axis: str, side: int) -> tuple[int, str, bool]:
    def outside(along: float) -> tuple[int, float, float]:
        epsilon = 1.0e-4
        u, v = (side * (1.0 + epsilon), along) if axis == "U" else (along, side * (1.0 + epsilon))
        return point_to_face(*face_point(face, u, v))

    first = outside(-0.5)
    second = outside(0.5)
    neighbor, u0, v0 = first
    _, u1, v1 = second
    if abs(abs(u0) - 1.0) < 0.01:
        target_edge = "U+" if u0 > 0 else "U-"
        along0, along1 = v0, v1
    else:
        target_edge = "V+" if v0 > 0 else "V-"
        along0, along1 = u0, u1
    return neighbor, target_edge, along1 < along0


def derive() -> dict[tuple[int, str], tuple[int, str, bool]]:
    return {
        (face, edge_name): derive_one(face, edge_name, axis, side)
        for face in range(6)
        for edge_name, axis, side in EDGES
    }


def read_table() -> dict[tuple[int, str], tuple[int, str, bool]]:
    document = tomllib.loads(TABLE.read_text(encoding="utf-8"))
    return {
        (item["face"], item["edge"]): (item["to_face"], item["to_edge"], item["flip"])
        for item in document["mapping"]
    }


def check_involution(table: dict[tuple[int, str], tuple[int, str, bool]]) -> None:
    for (face, edge_name), (neighbor, neighbor_edge, flip) in table.items():
        reverse = table[(neighbor, neighbor_edge)]
        if reverse != (face, edge_name, flip):
            raise SystemExit(
                f"non-involutive entry {face}.{edge_name}: "
                f"{neighbor}.{neighbor_edge} flip={flip}; reverse={reverse}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="compare the derived table to the committed table")
    args = parser.parse_args()
    derived = derive()
    table = read_table()
    check_involution(table)
    if args.check and derived != table:
        for key in sorted(derived):
            if derived.get(key) != table.get(key):
                print(f"{key[0]}.{key[1]}: derived={derived.get(key)} table={table.get(key)}")
        return 1
    print(f"PASS: derived {len(derived)} face edges; table matches; transforms are involutive")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
