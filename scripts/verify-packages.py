"""Verify actual release resource embedding, native identity, and Android ELF alignment."""
from pathlib import Path
import hashlib
import io
import json
import os
import re
import struct
import subprocess
import zipfile


project = Path(__file__).resolve().parent.parent
version = json.loads((project / "package.json").read_text(encoding="utf-8"))["version"]
release = project / "releases"
assets = re.findall(r"assets/([^\"']+)", (project / "dist/index.html").read_text(encoding="utf-8"))
if len(assets) < 2:
    raise RuntimeError("Production HTML lacks expected assets")
windows_path = release / f"CMCloudPlusPlus-{version}-windows-x64-portable.exe"
if not windows_path.is_file():
    windows_path = release / f"CMCloudPlusPlus-{version}-windows-x64.exe"
windows = windows_path.read_bytes()
apk_path = release / f"CMCloudPlusPlus-{version}-android-arm64-preview.apk"
with zipfile.ZipFile(apk_path) as apk:
    architectures = {name.split("/")[1] for name in apk.namelist() if name.startswith("lib/") and name.endswith(".so")}
    if architectures != {"arm64-v8a"}:
        raise RuntimeError(f"Unexpected APK architectures: {architectures}")
    library = apk.read("lib/arm64-v8a/libcmcloud_lib.so")
    sdk = Path(os.environ.get("ANDROID_HOME") or Path(os.environ["LOCALAPPDATA"]) / "Android/Sdk")
    build_tools = sorted((sdk / "build-tools").glob("*/aapt2.exe"),
                         key=lambda path: tuple(int(part) for part in re.findall(r"\d+", path.parent.name)))
    if not build_tools:
        raise RuntimeError("Android build-tools with aapt2 are required for launcher verification")
    resources = subprocess.run([str(build_tools[-1]), "dump", "resources", str(apk_path)],
                               check=True, capture_output=True, text=True, encoding="utf-8").stdout
    launcher_pngs = 0
    for name in ["ic_launcher", "ic_launcher_round"]:
        block = re.search(r"resource 0x[0-9a-f]+ mipmap/" + name + r"\s*\n((?:[ \t]+\([^\n]+\n)+)", resources)
        if not block:
            raise RuntimeError(f"APK lacks project launcher resource: {name}")
        for density in ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"]:
            entry = re.search(r"\(" + density + r"\) \(file\) (\S+) type=PNG", block[1])
            if not entry:
                raise RuntimeError(f"APK lacks {density} {name}")
            icon = project / "src-tauri/icons/android" / f"mipmap-{density}/{name}.png"
            packaged_image, source_image = apk.read(entry[1]), icon.read_bytes()
            if packaged_image != source_image:
                # AAPT can recompress PNGs or convert them to indexed color.
                # Compare decoded pixels rather than compression/metadata bytes.
                try:
                    from PIL import Image
                except ImportError as error:
                    raise RuntimeError("Install Pillow to verify AAPT-optimized launcher PNGs") from error
                with Image.open(io.BytesIO(packaged_image)) as packaged, Image.open(io.BytesIO(source_image)) as source_icon:
                    if packaged.size != source_icon.size or packaged.convert("RGBA").tobytes() != source_icon.convert("RGBA").tobytes():
                        raise RuntimeError(f"APK contains an outdated launcher image: {density} {name}")
            launcher_pngs += 1
        if not re.search(r"\(anydpi-v26\) \(file\) \S+ type=XML", block[1]):
            raise RuntimeError(f"APK lacks the adaptive launcher icon: {name}")
    foreground = re.search(r"resource (0x[0-9a-f]+) drawable/ic_launcher_foreground\s*\n\s*\(\) \(file\) (\S+) type=XML", resources)
    if not foreground:
        raise RuntimeError("APK lacks the safe-area launcher foreground vector")
    vector = subprocess.run([str(build_tools[-1]), "dump", "xmltree", "--file", foreground[2], str(apk_path)],
                            check=True, capture_output=True, text=True, encoding="utf-8").stdout
    if "scaleX" not in vector or "translateX" not in vector or "E: vector" not in vector:
        raise RuntimeError("APK launcher foreground lacks its safe-area transform")
target_directory = Path(os.environ.get("CARGO_TARGET_DIR", project / "src-tauri/target"))
source = (target_directory / "aarch64-linux-android/release/libcmcloud_lib.so").read_bytes()
if hashlib.sha256(library).digest() != hashlib.sha256(source).digest():
    raise RuntimeError("APK native library differs from Cargo output")
for target, binary in [("Windows", windows), ("Android", library)]:
    for asset in assets:
        if asset.encode("utf-8") not in binary:
            raise RuntimeError(f"{target} does not embed current asset {asset}")

if library[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", library, 18)[0] != 183:
    raise RuntimeError("Expected little-endian AArch64 ELF")
phoff = struct.unpack_from("<Q", library, 32)[0]
phsize, phcount = struct.unpack_from("<HH", library, 54)
loads = 0
for index in range(phcount):
    kind, flags, offset, address, physical, file_size, memory_size, alignment = struct.unpack_from("<IIQQQQQQ", library, phoff + phsize * index)
    if kind == 1:
        loads += 1
        if alignment < 16384 or (address - offset) % 16384:
            raise RuntimeError("ELF LOAD segment is not compatible with 16 KB pages")
    if kind == 0x6474E552 and (address + memory_size) % 16384:
        raise RuntimeError("GNU_RELRO end is not 16 KB aligned")
if not loads:
    raise RuntimeError("ELF lacks LOAD segments")
print(json.dumps({"version": version, "embeddedAssets": assets, "nativeSHA256": hashlib.sha256(library).hexdigest(), "loadSegments": loads, "alignment": "16 KB", "architectures": sorted(architectures), "verifiedLauncherImages": launcher_pngs, "adaptiveIcons": 2, "safeAreaForeground": True}, indent=2))
