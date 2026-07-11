#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from PIL import Image, ImageDraw

BORDER_RATIO = 0.06

# Opaque background painted behind the (transparent) logo on any icon that is
# shown against a surface rather than overlaid on the OS (favicons, app icons).
# Override per run with --background "#rrggbb".
ICON_BACKGROUND_HEX = "#f4efe3"
# Corner radius of "rounded" backgrounds, as a fraction of the icon side. ~0.22
# matches the iOS/macOS squircle look.
ROUNDED_RADIUS_RATIO = 0.22
# Logo size as a fraction of the icon side when it sits on a background, leaving
# a margin so the artwork doesn't crowd the edge. (The master already carries a
# ~6% transparent border, so the visible art is a bit smaller than these.)
CONTENT_SCALE_ROUNDED = 0.80
CONTENT_SCALE_SQUARE = 0.84
CONTENT_SCALE_CIRCLE = 0.72
# Supersampling factor used when rasterizing rounded/circle masks so their
# edges stay smooth even at favicon sizes.
MASK_SUPERSAMPLE = 4
IOS_APP_ICON_SPECS = (
    {"idiom": "iphone", "size": "20x20", "scale": "2x", "pixels": 40},
    {"idiom": "iphone", "size": "20x20", "scale": "3x", "pixels": 60},
    {"idiom": "iphone", "size": "29x29", "scale": "2x", "pixels": 58},
    {"idiom": "iphone", "size": "29x29", "scale": "3x", "pixels": 87},
    {"idiom": "iphone", "size": "40x40", "scale": "2x", "pixels": 80},
    {"idiom": "iphone", "size": "40x40", "scale": "3x", "pixels": 120},
    {"idiom": "iphone", "size": "60x60", "scale": "2x", "pixels": 120},
    {"idiom": "iphone", "size": "60x60", "scale": "3x", "pixels": 180},
    {"idiom": "ipad", "size": "20x20", "scale": "1x", "pixels": 20},
    {"idiom": "ipad", "size": "20x20", "scale": "2x", "pixels": 40},
    {"idiom": "ipad", "size": "29x29", "scale": "1x", "pixels": 29},
    {"idiom": "ipad", "size": "29x29", "scale": "2x", "pixels": 58},
    {"idiom": "ipad", "size": "40x40", "scale": "1x", "pixels": 40},
    {"idiom": "ipad", "size": "40x40", "scale": "2x", "pixels": 80},
    {"idiom": "ipad", "size": "76x76", "scale": "1x", "pixels": 76},
    {"idiom": "ipad", "size": "76x76", "scale": "2x", "pixels": 152},
    {"idiom": "ipad", "size": "83.5x83.5", "scale": "2x", "pixels": 167},
    {"idiom": "ios-marketing", "size": "1024x1024", "scale": "1x", "pixels": 1024},
)


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


def save_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")


def copy_file(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def remove_file_if_exists(path: Path) -> None:
    if path.exists():
        path.unlink()


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
    # No recoloring
    return raw;
    # alpha = raw.getchannel("A")
    # recolored = Image.new("RGBA", raw.size, (*rgb, 0))
    # recolored.putalpha(alpha)
    # return recolored


def make_monochrome_black(image: Image.Image) -> Image.Image:
    """Return a black+alpha version of the image (macOS template image format)."""
    alpha = image.getchannel("A")
    mono = Image.new("RGBA", image.size, (0, 0, 0, 0))
    mono.putalpha(alpha)
    return mono


@dataclass(frozen=True)
class Background:
    """An opaque fill painted behind the logo.

    shape:         "square" (full bleed), "rounded" (squircle), or "circle".
    content_scale: logo size relative to the icon side, to keep the artwork
                   inside the visible region (e.g. away from a circle's edge).
    """

    color: tuple[int, int, int]
    shape: str = "square"
    content_scale: float = 1.0


def _shape_mask(size: int, shape: str) -> Image.Image | None:
    """Anti-aliased alpha mask for a non-square background, or None for square."""
    if shape == "square":
        return None
    hi = size * MASK_SUPERSAMPLE
    mask = Image.new("L", (hi, hi), 0)
    draw = ImageDraw.Draw(mask)
    if shape == "circle":
        draw.ellipse((0, 0, hi - 1, hi - 1), fill=255)
    elif shape == "rounded":
        radius = max(1, round(hi * ROUNDED_RADIUS_RATIO))
        draw.rounded_rectangle((0, 0, hi - 1, hi - 1), radius=radius, fill=255)
    else:
        raise ValueError(f"unknown background shape: {shape!r}")
    return mask.resize((size, size), Image.Resampling.LANCZOS)


def render_master(
    master: Image.Image, size: int, background: Background | None
) -> Image.Image:
    """Resize the logo to `size`, optionally compositing it onto a background."""
    content_scale = background.content_scale if background else 1.0
    content_size = max(1, round(size * content_scale))
    content = master.resize((content_size, content_size), Image.Resampling.LANCZOS)
    offset = (size - content_size) // 2

    if background is None:
        if content_size == size:
            return content
        base = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    else:
        base = Image.new("RGBA", (size, size), (*background.color, 255))
        mask = _shape_mask(size, background.shape)
        if mask is not None:
            base.putalpha(mask)

    base.alpha_composite(content, (offset, offset))
    return base


def save_png(
    master: Image.Image, path: Path, size: int, background: Background | None = None
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    render_master(master, size, background).save(path, format="PNG", optimize=True)


def save_png_with_canvas(
    master: Image.Image,
    path: Path,
    width: int,
    height: int,
    content_size: int,
    background: Background | None = None,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    content = render_master(master, content_size, background)
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    x = (width - content_size) // 2
    y = (height - content_size) // 2
    canvas.alpha_composite(content, (x, y))
    canvas.save(path, format="PNG", optimize=True)


def save_ico(
    master: Image.Image,
    path: Path,
    sizes: Iterable[int],
    background: Background | None = None,
) -> None:
    sorted_sizes = sorted(set(sizes))
    path.parent.mkdir(parents=True, exist_ok=True)
    out = render_master(master, sorted_sizes[-1], background)
    out.save(path, format="ICO", sizes=[(size, size) for size in sorted_sizes])


def save_icns(
    master: Image.Image,
    path: Path,
    sizes: Iterable[int],
    background: Background | None = None,
) -> None:
    sorted_sizes = sorted(set(sizes))
    path.parent.mkdir(parents=True, exist_ok=True)
    out = render_master(master, sorted_sizes[-1], background)
    out.save(path, format="ICNS", sizes=[(size, size) for size in sorted_sizes])


def ios_icon_filename(spec: dict[str, str | int]) -> str:
    idiom = str(spec["idiom"]).replace("ios-", "")
    size = str(spec["size"])
    scale = str(spec["scale"])
    return f"Icon-App-{idiom}-{size}@{scale}.png"


def save_ios_app_icons(
    master: Image.Image, assets_dir: Path, background: Background
) -> list[Path]:
    app_icon_dir = assets_dir / "AppIcon.appiconset"
    outputs = [assets_dir / "Contents.json", app_icon_dir / "Contents.json"]

    if app_icon_dir.exists():
        shutil.rmtree(app_icon_dir)

    save_json(
        assets_dir / "Contents.json",
        {
            "info": {
                "author": "xcode",
                "version": 1,
            }
        },
    )

    contents_images = []
    for spec in IOS_APP_ICON_SPECS:
        filename = ios_icon_filename(spec)
        target_path = app_icon_dir / filename
        # iOS requires fully opaque icons and applies its own corner mask, so
        # these are full-bleed squares rather than pre-rounded.
        save_png(master, target_path, int(spec["pixels"]), background)
        outputs.append(target_path)
        contents_images.append(
            {
                "filename": filename,
                "idiom": str(spec["idiom"]),
                "scale": str(spec["scale"]),
                "size": str(spec["size"]),
            }
        )

    save_json(
        app_icon_dir / "Contents.json",
        {
            "images": contents_images,
            "info": {
                "author": "xcode",
                "version": 1,
            },
        },
    )
    return outputs


MAC_APP_ICON_SPECS = (
    {"size": "16x16", "scale": "1x", "pixels": 16},
    {"size": "16x16", "scale": "2x", "pixels": 32},
    {"size": "32x32", "scale": "1x", "pixels": 32},
    {"size": "32x32", "scale": "2x", "pixels": 64},
    {"size": "128x128", "scale": "1x", "pixels": 128},
    {"size": "128x128", "scale": "2x", "pixels": 256},
    {"size": "256x256", "scale": "1x", "pixels": 256},
    {"size": "256x256", "scale": "2x", "pixels": 512},
    {"size": "512x512", "scale": "1x", "pixels": 512},
    {"size": "512x512", "scale": "2x", "pixels": 1024},
)


def save_asset_catalog_contents(assets_dir: Path) -> Path:
    path = assets_dir / "Contents.json"
    save_json(path, {"info": {"author": "xcode", "version": 1}})
    return path


def save_mac_app_icons(
    master: Image.Image, assets_dir: Path, background: Background
) -> list[Path]:
    app_icon_dir = assets_dir / "AppIcon.appiconset"
    outputs = [save_asset_catalog_contents(assets_dir), app_icon_dir / "Contents.json"]

    if app_icon_dir.exists():
        shutil.rmtree(app_icon_dir)

    contents_images = []
    for spec in MAC_APP_ICON_SPECS:
        filename = f"icon_{spec['size']}.png" if spec["scale"] == "1x" else f"icon_{spec['size']}@{spec['scale']}.png"
        target_path = app_icon_dir / filename
        # macOS does not mask app icons, so bake the squircle in, matching
        # the standalone AppIcon.icns treatment below.
        save_png(master, target_path, int(spec["pixels"]), background)
        outputs.append(target_path)
        contents_images.append(
            {
                "filename": filename,
                "idiom": "mac",
                "scale": str(spec["scale"]),
                "size": str(spec["size"]),
            }
        )

    save_json(
        app_icon_dir / "Contents.json",
        {
            "images": contents_images,
            "info": {"author": "xcode", "version": 1},
        },
    )
    return outputs


def save_mac_tray_icon(master: Image.Image, assets_dir: Path) -> list[Path]:
    tray_icon_dir = assets_dir / "TrayIcon.imageset"
    outputs = [save_asset_catalog_contents(assets_dir), tray_icon_dir / "Contents.json"]

    if tray_icon_dir.exists():
        shutil.rmtree(tray_icon_dir)

    mono = make_monochrome_black(master)
    sizes = ({"scale": "1x", "pixels": 16}, {"scale": "2x", "pixels": 32})
    contents_images = []
    for spec in sizes:
        filename = f"tray-icon-{spec['pixels']}.png"
        target_path = tray_icon_dir / filename
        save_png(mono, target_path, int(spec["pixels"]))
        outputs.append(target_path)
        contents_images.append(
            {"filename": filename, "idiom": "mac", "scale": str(spec["scale"])}
        )

    save_json(
        tray_icon_dir / "Contents.json",
        {
            "images": contents_images,
            "info": {"author": "xcode", "version": 1},
            # Lets the menu bar tint it automatically for light/dark mode,
            # matching the standalone tray-icon.png used by the old AppKit UI.
            "properties": {"template-rendering-intent": "template"},
        },
    )
    return outputs


def preprocess_logo(
    raw_path: Path, out_path: Path | None, theme_rgb: tuple[int, int, int]
) -> Image.Image:
    raw = Image.open(raw_path).convert("RGBA")
    raw = recolor_with_theme(raw, theme_rgb)

    if raw.getbbox() is None:
        raise SystemExit(f"{raw_path} has no non-transparent pixels")

    # Preserve the source framing: we never crop to the artwork's bounding box,
    # so any padding baked into the SVG/export survives. We only pad a non-square
    # source to a square, then scale the whole thing down to leave a uniform
    # transparent border (the script's own padding).
    side = max(raw.width, raw.height)
    if raw.width != raw.height:
        squared = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        squared.alpha_composite(raw, ((side - raw.width) // 2, (side - raw.height) // 2))
        raw = squared

    content_target = max(1, int(round(side * (1.0 - (2.0 * BORDER_RATIO)))))
    scaled = raw.resize((content_target, content_target), Image.Resampling.LANCZOS)

    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    offset = (side - content_target) // 2
    canvas.alpha_composite(scaled, (offset, offset))

    if out_path is not None:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        canvas.save(out_path, format="PNG", optimize=True)
    return canvas


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--target",
        choices=("all", "ios", "mac"),
        default="all",
        help="Limit generated outputs to a specific target set.",
    )
    parser.add_argument(
        "--background",
        default=ICON_BACKGROUND_HEX,
        help=(
            "Opaque background color (#rrggbb) painted behind the logo on "
            f"favicons and app icons. Defaults to {ICON_BACKGROUND_HEX}."
        ),
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    root = Path(__file__).resolve().parent.parent
    images_dir = root / "images"
    raw_path = images_dir / "logo-raw.png"
    svg_path = images_dir / "logo.svg"
    prepped_path = images_dir / "logo-prepped.png"
    theme_rgb = load_theme_color(root)

    try:
        background_rgb = parse_hex_color(args.background)
    except ValueError as exc:
        raise SystemExit(f"invalid --background color: {exc}") from exc

    # Full-bleed square for icons the OS masks itself (iOS, Android, PWA), a
    # squircle for standalone icons shown as-is (favicons, desktop apps), and a
    # circle for Android's round launcher icon and the Linux tray icon (which
    # sits on a themed surface but needs its own background to stay visible in
    # dark mode). Windows tiles are left transparent (the OS provides a plate).
    rounded_bg = Background(background_rgb, "rounded", CONTENT_SCALE_ROUNDED)
    square_bg = Background(background_rgb, "square", CONTENT_SCALE_SQUARE)
    circle_bg = Background(background_rgb, "circle", CONTENT_SCALE_CIRCLE)

    if not raw_path.exists():
        raise SystemExit(f"missing source image: {raw_path}")
    if not svg_path.exists():
        raise SystemExit(f"missing source svg: {svg_path}")

    master = preprocess_logo(
        raw_path,
        prepped_path if args.target == "all" else None,
        theme_rgb,
    )

    outputs: list[Path] = [prepped_path] if args.target == "all" else []

    if args.target == "all":
        web_public = root / "web" / "public"
        landing_public = root / "landing" / "public"
        remove_file_if_exists(web_public / "logo.svg")
        copy_file(svg_path, landing_public / "logo.svg")

        for path in [web_public, landing_public]:
            save_ico(master, path / "favicon.ico", [16, 32, 48], rounded_bg)
            save_png(master, path / "favicon-16x16.png", 16, rounded_bg)
            save_png(master, path / "favicon-32x32.png", 32, rounded_bg)
            # apple-touch and android-chrome are masked by the OS / browser, so
            # they use a full-bleed square background.
            save_png(master, path / "apple-touch-icon.png", 180, square_bg)
            save_png(master, path / "android-chrome-192x192.png", 192, square_bg)
            save_png(master, path / "android-chrome-512x512.png", 512, square_bg)
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

        outputs.append(landing_public / "logo.svg")

        linux_assets = root / "client" / "linux" / "assets"
        save_png(master, linux_assets / "tray-icon.png", 32, circle_bg)
        outputs.append(linux_assets / "tray-icon.png")

        windows_assets = root / "client" / "windows" / "assets"
        # app-icon is the window/taskbar icon and gets a background. The Store
        # tiles and *_altform-unplated images stay transparent: Windows draws a
        # themed plate behind tiles, and "unplated" means no plate at all.
        save_ico(
            master,
            windows_assets / "app-icon.ico",
            [16, 24, 32, 40, 48, 64, 128, 256],
            rounded_bg,
        )
        save_png(master, windows_assets / "app-icon.png", 256, rounded_bg)
        save_png(master, windows_assets / "Square44x44Logo.png", 44)
        save_png(master, windows_assets / "Square150x150Logo.png", 150)
        save_png(master, windows_assets / "StoreLogo.png", 50)
        save_png_with_canvas(master, windows_assets / "SplashScreen.png", 620, 300, 220)
        for size in [16, 20, 24, 32, 40, 48, 64, 256]:
            save_png(
                master,
                windows_assets
                / f"Square44x44Logo.targetsize-{size}_altform-unplated.png",
                size,
            )
        outputs.extend(
            [
                windows_assets / "app-icon.ico",
                windows_assets / "app-icon.png",
                windows_assets / "Square44x44Logo.png",
                windows_assets / "Square150x150Logo.png",
                windows_assets / "StoreLogo.png",
                windows_assets / "SplashScreen.png",
            ]
            + [
                windows_assets / f"Square44x44Logo.targetsize-{size}_altform-unplated.png"
                for size in [16, 20, 24, 32, 40, 48, 64, 256]
            ]
        )

        android_base = root / "client" / "android" / "app" / "src" / "main" / "res"
        android_sizes = {
            "mipmap-mdpi": 48,
            "mipmap-hdpi": 72,
            "mipmap-xhdpi": 96,
            "mipmap-xxhdpi": 144,
            "mipmap-xxxhdpi": 192,
        }
        for bucket, size in android_sizes.items():
            # No adaptive-icon XML here, so the PNGs are shown directly: a
            # squircle for the standard icon and a true circle for the round one.
            save_png(master, android_base / bucket / "ic_launcher.png", size, rounded_bg)
            save_png(
                master,
                android_base / bucket / "ic_launcher_round.png",
                size,
                circle_bg,
            )
            outputs.append(android_base / bucket / "ic_launcher.png")
            outputs.append(android_base / bucket / "ic_launcher_round.png")

    if args.target in ("all", "mac"):
        mac_assets = root / "client" / "mac" / "assets"
        # macOS does not mask app icons, so bake the squircle in. The tray icon
        # stays transparent so it tints with the menu bar.
        save_icns(
            master,
            mac_assets / "AppIcon.icns",
            [16, 32, 64, 128, 256, 512, 1024],
            rounded_bg,
        )
        save_png(make_monochrome_black(master), mac_assets / "tray-icon.png", 32)
        outputs.extend([mac_assets / "AppIcon.icns", mac_assets / "tray-icon.png"])

        mac_swiftui_assets = root / "client" / "mac" / "app" / "Assets.xcassets"
        outputs.extend(save_mac_app_icons(master, mac_swiftui_assets, rounded_bg))
        outputs.extend(save_mac_tray_icon(master, mac_swiftui_assets))

    if args.target in ("all", "ios"):
        ios_assets = root / "client" / "ios" / "app" / "Assets.xcassets"
        outputs.extend(save_ios_app_icons(master, ios_assets, square_bg))

    print("Generated icon assets:")
    for path in outputs:
        print(f"- {rel(path, root)}")


if __name__ == "__main__":
    main()
