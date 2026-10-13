"""Shared collection of resolved Rust dependency license texts."""
import hashlib
from pathlib import Path
import re
import shutil


def collect(packages: list[dict], destination: Path, apache: Path, apache_sha256: str) -> list[dict]:
    if hashlib.sha256(apache.read_bytes()).hexdigest() != apache_sha256:
        raise ValueError("Pinned Apache license text changed")
    destination.mkdir(parents=True, exist_ok=True)
    records = []
    for package in packages:
        if package["source"] is None:
            continue
        source = Path(package["manifest_path"]).parent.resolve(strict=True)
        name = f"{package['name']}-{package['version']}"
        if Path(name).name != name or "\\" in name:
            raise ValueError("Invalid dependency name or version")
        target = destination / name
        texts = []

        def copy(notice: Path) -> None:
            if not notice.resolve(strict=True).is_relative_to(source):
                raise ValueError("Dependency license escapes its source directory")
            relative = notice.relative_to(source).as_posix()
            if ".." in Path(relative).parts:
                raise ValueError("Dependency license path is not normalized")
            if relative in texts:
                return
            output = target / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(notice, output)
            texts.append(relative)

        for notice in sorted(source.rglob("*")):
            if notice.is_file() and notice.name.lower().startswith(("license", "licence", "copying", "notice")):
                copy(notice)
        if package.get("license_file"):
            declared = source / package["license_file"]
            if declared.is_file():
                copy(declared)
        selected = None
        if not texts:
            # 仅在声明允许独立选择 Apache 时复用已校验文本；AND 义务不能被单个许可替代。
            expression = " ".join((package.get("license") or "").split())
            # 旧 Cargo 清单使用 '/' 表达任选其一，保留该历史格式，但不推断 AND/WITH 组合。
            options = expression.replace("(", "").replace(")", "").replace("/", " OR ").split(" OR ")
            options = [option.strip() for option in options]
            if (" AND " in expression or " WITH " in expression or "Apache-2.0" not in options or
                    any(not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.+-]*", option) for option in options)):
                raise ValueError(f"Missing license text for {package['name']}")
            target.mkdir(parents=True, exist_ok=True)
            shutil.copy2(apache, target / "Apache-2.0.txt")
            shutil.copy2(source / "Cargo.toml", target / "upstream-Cargo.toml")
            texts = ["Apache-2.0.txt", "upstream-Cargo.toml"]
            selected = "Apache-2.0"
        records.append({"name": package["name"], "version": package["version"],
                        "license": package.get("license"), "selected_license": selected,
                        "authors": package["authors"], "repository": package.get("repository"), "texts": texts})
    return records
