"""Package the client source and build instructions using Python's standard library."""

import json
import os
from pathlib import Path
import zipfile


project = Path(__file__).resolve().parent.parent
version = json.loads((project / "package.json").read_text(encoding="utf-8"))["version"]
root_files = (
    ".gitignore", ".prettierrc.json", "README.md", "DESIGN.md", "PRODUCT.md",
    "DRIVER_PARITY.md", "NOTICE.md", "LICENSE", "package.json", "package-lock.json",
    "index.html", "tsconfig.json", "vite.config.ts", "CONTRIBUTING.md",
)
source_dirs = ("src", "src-tauri", "scripts", "docs", ".impeccable")
excluded_dirs = {"target", "gen", "build", ".gradle", ".tauri", "__pycache__", "review"}
files = [project / name for name in root_files]
for source_dir in source_dirs:
    for parent, dirs, names in os.walk(project / source_dir, followlinks=False):
        dirs[:] = sorted(name for name in dirs if name not in excluded_dirs)
        files.extend(Path(parent) / name for name in sorted(names))

for source in files:
    if not source.is_file() or source.is_symlink():
        raise RuntimeError(f"Missing or unsupported source file: {source}")

output = project / "releases" / f"CMCloudPlusPlus-{version}-source.zip"
output.parent.mkdir(parents=True, exist_ok=True)
archive_root = f"CMCloudPlusPlus-{version}"
with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for source in sorted(files):
        archive.write(source, f"{archive_root}/{source.relative_to(project).as_posix()}")

with zipfile.ZipFile(output) as archive:
    damaged = archive.testzip()
    if damaged:
        raise RuntimeError(f"Archive checksum failed: {damaged}")
    for source in files:
        archived = archive.read(f"{archive_root}/{source.relative_to(project).as_posix()}")
        if archived != source.read_bytes():
            raise RuntimeError(f"Archive source verification failed: {source}")

print(f"{output}\n{len(files)} source files; {output.stat().st_size:,} bytes; verified")
