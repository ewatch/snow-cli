#!/usr/bin/env python3
"""Stage a snow-cli skill bundle for static HTTP hosting with SHA-256 [[files]] entries."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tomllib
from pathlib import Path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def has_dot_component(rel: Path) -> bool:
    return any(part.startswith(".") for part in rel.parts)


def collect_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        if has_dot_component(rel):
            continue
        if path.is_symlink():
            raise SystemExit(f"Refusing to stage symlink: {path}")
        if path.is_file():
            files.append(path)
    return files


def ensure_safe_output_dir(source: Path, out: Path) -> None:
    src_resolved = source.resolve()
    out_resolved = out.resolve()
    if (
        out_resolved == src_resolved
        or src_resolved in out_resolved.parents
        or out_resolved in src_resolved.parents
    ):
        raise SystemExit(
            f"Refusing to write staged output to {out} because it overlaps source directory {source}."
        )


def stage_bundle(source: Path, out: Path) -> int:
    manifest_src = source / "skill.toml"
    if not manifest_src.is_file():
        raise SystemExit(f"Missing skill manifest: {manifest_src}")

    ensure_safe_output_dir(source, out)

    manifest_text = manifest_src.read_text(encoding="utf-8")
    parsed = tomllib.loads(manifest_text)
    if "files" in parsed:
        raise SystemExit(
            f"Source manifest {manifest_src} already contains [[files]]; "
            "keep source skill.toml free of generated digests."
        )

    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True, exist_ok=True)

    entries: list[tuple[str, str]] = []
    for src_file in collect_files(source):
        rel = src_file.relative_to(source).as_posix()
        if rel == "skill.toml":
            continue
        dst_file = out / rel
        dst_file.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src_file, dst_file)
        entries.append((rel, sha256_file(src_file)))

    lines = [manifest_text.rstrip(), ""]
    for rel, digest in entries:
        lines.append("[[files]]")
        lines.append(f"path = {json.dumps(rel)}")
        lines.append(f"sha256 = {json.dumps(digest)}")
        lines.append("")

    (out / "skill.toml").write_text("\n".join(lines), encoding="utf-8")
    return len(entries)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="Source skill directory")
    parser.add_argument("--out", required=True, type=Path, help="Destination directory for staged files")
    args = parser.parse_args(argv)

    count = stage_bundle(args.source, args.out)
    print(f"Staged {count} skill files + manifest into {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
