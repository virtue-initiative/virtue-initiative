#!/usr/bin/env python3
from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Iterable

from PIL import Image

BORDER_RATIO = 0.06
TRIM_ALPHA_THRESHOLD = 2


def rel(path: Path, root: Path) -> str:
    return str(path.relative_to(root))


def parse_hex_color(value: str) -> tuple[int, int, int]:
    raw = value.strip()
    if raw.startswith("#"):
        raw = raw[1:]
    if len(raw) == 3:
        raw = "".join(ch * 2 for ch in raw)
    if re.fullmatch(r"[0-9a-fA-F]{6}", raw) is None:
        raise ValueError(f"expected #RRGGBB, got {value!r}")
    return tuple(int(raw[i : i + 2], 16) for i in (0, 2, 4))


def load_theme_color(root: Path) -> tuple[int, int, int]:
    theme_path = root / "theme.json"
    if not theme_path.exists():
        raise SystemExit(f"missing theme file: {theme_path}")

    try:
        with theme_path.open("r", encoding="utf-8") as handle:
            theme = json.load(handle)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"invalid JSON in {theme_path}: {exc}") from exc

    app_theme = theme.get("app")
    if not isinstance(app_theme, dict):
        raise SystemExit(f"missing app object in {theme_path}")

    color_value = app_theme.get("primaryColor")
    if not isinstance(color_value, str):
        raise SystemExit(f"missing app.primaryColor string in {theme_path}")

    try:
        return parse_hex_color(color_value)
    except ValueError as exc:
        raise SystemExit(f"invalid app.primaryColor in {theme_path}: {exc}") from exc


def recolor_with_theme(raw: Image.Image, rgb: tuple[int, int, int]) -> Image.Image:
    alpha = raw.getchannel("A")
    recolored = Image.new("RGBA", raw.size, (*rgb, 0))
    recolored.putalpha(alpha)
    return recolored


def save_png(master: Image.Image, path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    out = master.resize((size, size), Image.Resampling.LANCZOS)
    out.save(path, format="PNG", optimize=True)


def save_ico(master: Image.Image, path: Path, sizes: Iterable[int]) -> None:
    sorted_sizes = sorted(set(sizes))
    path.parent.mkdir(parents=True, exist_ok=True)
    out = master.resize((sorted_sizes[-1], sorted_sizes[-1]), Image.Resampling.LANCZOS)
    out.save(path, format="ICO", sizes=[(size, size) for size in sorted_sizes])


def save_icns(master: Image.Image, path: Path, sizes: Iterable[int]) -> None:
    sorted_sizes = sorted(set(sizes))
    path.parent.mkdir(parents=True, exist_ok=True)
    out = master.resize((sorted_sizes[-1], sorted_sizes[-1]), Image.Resampling.LANCZOS)
    out.save(path, format="ICNS", sizes=[(size, size) for size in sorted_sizes])


def preprocess_logo(raw_path: Path, out_path: Path, theme_rgb: tuple[int, int, int]) -> Image.Image:
    raw = Image.open(raw_path).convert("RGBA")
    raw = recolor_with_theme(raw, theme_rgb)
    alpha = raw.getchannel("A")
    mask = alpha.point(
        lambda x: 255 if x >= TRIM_ALPHA_THRESHOLD else 0,
        mode="L",
    )
    bbox = mask.getbbox()
    if bbox is None:
        raise SystemExit(f"{raw_path} has no non-transparent pixels")

    trimmed = raw.crop(bbox)

    side = max(raw.width, raw.height)
    content_target = max(1, int(round(side * (1.0 - (2.0 * BORDER_RATIO)))))
    scale = min(content_target / trimmed.width, content_target / trimmed.height)
    scaled_size = (
        max(1, int(round(trimmed.width * scale))),
        max(1, int(round(trimmed.height * scale))),
    )
    trimmed = trimmed.resize(scaled_size, Image.Resampling.LANCZOS)

    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    x = (side - scaled_size[0]) // 2
    y = (side - scaled_size[1]) // 2
    canvas.paste(trimmed, (x, y), trimmed)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out_path, format="PNG", optimize=True)
    return canvas


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    images_dir = root / "images"
    raw_path = images_dir / "logo-raw.png"
    prepped_path = images_dir / "logo-prepped.png"
    theme_rgb = load_theme_color(root)

    if not raw_path.exists():
        raise SystemExit(f"missing source image: {raw_path}")

    master = preprocess_logo(raw_path, prepped_path, theme_rgb)

    outputs: list[Path] = [prepped_path]

    for path in [root / "web" / "public", root / "help" / "static", root / "landing"]:
        save_ico(master, path / "favicon.ico", [16, 32, 48])
        save_png(master, path / "favicon-16x16.png", 16)
        save_png(master, path / "favicon-32x32.png", 32)
        save_png(master, path / "apple-touch-icon.png", 180)
        save_png(master, path / "android-chrome-192x192.png", 192)
        save_png(master, path / "android-chrome-512x512.png", 512)
        outputs.extend(
            [
                path / "favicon.ico",
                path / "favicon-16x16.png",
                path / "favicon-32x32.png",
                path / "apple-touch-icon.png",
                path / "android-chrome-192x192.png",
                path / "android-chrome-512x512.png",
            ]
        )

    mac_assets = root / "client" / "mac" / "assets"
    save_icns(master, mac_assets / "AppIcon.icns", [16, 32, 64, 128, 256, 512, 1024])
    save_png(master, mac_assets / "tray-icon.png", 32)
    outputs.extend([mac_assets / "AppIcon.icns", mac_assets / "tray-icon.png"])

    linux_assets = root / "client" / "linux" / "assets"
    save_png(master, linux_assets / "tray-icon.png", 32)
    outputs.append(linux_assets / "tray-icon.png")

    windows_assets = root / "client" / "windows" / "assets"
    save_ico(master, windows_assets / "app-icon.ico", [16, 24, 32, 40, 48, 64, 128, 256])
    save_png(master, windows_assets / "app-icon.png", 256)
    outputs.extend([windows_assets / "app-icon.ico", windows_assets / "app-icon.png"])

    android_base = root / "client" / "android" / "app" / "src" / "main" / "res"
    android_sizes = {
        "mipmap-mdpi": 48,
        "mipmap-hdpi": 72,
        "mipmap-xhdpi": 96,
        "mipmap-xxhdpi": 144,
        "mipmap-xxxhdpi": 192,
    }
    for bucket, size in android_sizes.items():
        save_png(master, android_base / bucket / "ic_launcher.png", size)
        save_png(master, android_base / bucket / "ic_launcher_round.png", size)
        outputs.append(android_base / bucket / "ic_launcher.png")
        outputs.append(android_base / bucket / "ic_launcher_round.png")

    print("Generated icon assets:")
    for path in outputs:
        print(f"- {rel(path, root)}")


if __name__ == "__main__":
    main()
