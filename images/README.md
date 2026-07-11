# Icon Pipeline

Source image:

- `images/logo-raw.png` (exported from `images/logo.svg`).
- RGB is normalized to `app.primaryColor` from `theme.json` while preserving each pixel's original alpha channel.
- The artwork is **not** cropped, so the framing/padding you set in the SVG (its `viewBox` and the logo's position within it) is preserved into every icon. To add more padding, give the logo more margin in `logo.svg` and re-export `logo-raw.png`; the script adds its own border/inset on top of that.

Regeneration:

- `./images/generate-icons.sh`
- `./images/generate-icons.sh --target ios` to regenerate only the iOS app icon set
- `./images/generate-icons.sh --target mac` to regenerate only the mac app icon/tray icon set
- `./images/generate-icons.sh --background "#rrggbb"` to override the icon background color (default `#f4efe3`)

Requirements:

- `python3` with Pillow (`PIL`)

What the script does:

- Loads app theme color from `theme.json` (`app.primaryColor`).
- Recolors `logo-raw.png` to that theme color, preserving alpha.
- Pads the source to a square (if needed) and scales it down to leave a uniform transparent border, writing `images/logo-prepped.png`. The source framing is preserved — nothing is cropped to the artwork's bounding box.
- Paints an opaque background (`#f4efe3` by default) behind the logo on any icon shown against a surface, choosing the shape per target:
  - **Rounded (squircle):** favicons and standalone desktop icons that the OS shows as-is — `favicon.*`, mac `AppIcon.icns`, windows `app-icon.*`, android `ic_launcher.png`.
  - **Square (full bleed):** icons the OS/browser masks itself — iOS app icons, `apple-touch-icon.png`, `android-chrome-*`.
  - **Circle:** android `ic_launcher_round.png`.
  - **Transparent (no background):** tray icons, the Windows splash, and the Windows Store tiles / `*_altform-unplated` images, which sit on a system-provided surface.
- Generates and overwrites derived icons used by web and client targets.

Generated targets:

- `web/public/favicon.ico`
- `web/public/favicon-16x16.png`
- `web/public/favicon-32x32.png`
- `web/public/apple-touch-icon.png`
- `web/public/android-chrome-192x192.png`
- `web/public/android-chrome-512x512.png`
- `landing/public/logo.svg`
- `client/mac/assets/AppIcon.icns`
- `client/mac/assets/tray-icon.png`
- `client/mac/app/Assets.xcassets/Contents.json`
- `client/mac/app/Assets.xcassets/AppIcon.appiconset/*`
- `client/mac/app/Assets.xcassets/TrayIcon.imageset/*`
- `client/linux/assets/tray-icon.png`
- `client/windows/assets/app-icon.ico`
- `client/windows/assets/app-icon.png`
- `client/android/app/src/main/res/mipmap-*/ic_launcher.png`
- `client/android/app/src/main/res/mipmap-*/ic_launcher_round.png`
- `client/ios/app/Assets.xcassets/Contents.json`
- `client/ios/app/Assets.xcassets/AppIcon.appiconset/*`
