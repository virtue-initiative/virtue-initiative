# Icon Pipeline

Source image:

- `images/logo-raw.png`
- RGB is normalized to `app.primaryColor` from `theme.json` while preserving each pixel's original alpha channel.

Regeneration:

- `./images/generate-icons.sh`

Requirements:

- `python3` with Pillow (`PIL`)

What the script does:

- Loads app theme color from `theme.json` (`app.primaryColor`).
- Recolors `logo-raw.png` to that theme color, preserving alpha.
- Crops transparent margins from `logo-raw.png` using a tiny alpha-noise threshold.
- Builds a square, centered `images/logo-prepped.png` with a small transparent border.
- Generates and overwrites derived icons used by web and client targets.

Generated targets:

- `web/public/favicon.ico`
- `web/public/favicon-16x16.png`
- `web/public/favicon-32x32.png`
- `web/public/apple-touch-icon.png`
- `web/public/android-chrome-192x192.png`
- `web/public/android-chrome-512x512.png`
- `client/mac/assets/AppIcon.icns`
- `client/mac/assets/tray-icon.png`
- `client/linux/assets/tray-icon.png`
- `client/windows/assets/app-icon.ico`
- `client/windows/assets/app-icon.png`
- `client/android/app/src/main/res/mipmap-*/ic_launcher.png`
- `client/android/app/src/main/res/mipmap-*/ic_launcher_round.png`
