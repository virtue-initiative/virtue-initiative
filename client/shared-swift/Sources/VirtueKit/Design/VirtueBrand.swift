import SwiftUI

/// Colors matching `shared-web/tokens.css`, shared by the iOS and Mac apps.
public enum VirtueBrand {
    /// Forest green — matches --accent / --forest.
    public static let accent = Color(red: 30.0 / 255.0, green: 58.0 / 255.0, blue: 46.0 / 255.0)
    /// Link color — matches --link.
    public static let link = Color(red: 179.0 / 255.0, green: 67.0 / 255.0, blue: 0.0 / 255.0)
    /// Warm ochre — matches --ochre.
    public static let ochre = Color(red: 166.0 / 255.0, green: 127.0 / 255.0, blue: 61.0 / 255.0)
    /// Page background — matches --bg (#f4efe3).
    public static let bg = Color(red: 244.0 / 255.0, green: 239.0 / 255.0, blue: 227.0 / 255.0)
    /// Card surface — matches --surface (#fbf7ea).
    public static let surface = Color(red: 251.0 / 255.0, green: 247.0 / 255.0, blue: 234.0 / 255.0)
    /// Subtle background — matches --bg-subtle (#ebe4ce).
    public static let bgSubtle = Color(red: 235.0 / 255.0, green: 228.0 / 255.0, blue: 206.0 / 255.0)
    /// Border — matches --border (#d9d1bc).
    public static let border = Color(red: 217.0 / 255.0, green: 209.0 / 255.0, blue: 188.0 / 255.0)
    /// Primary text — matches --text (#1b1a16).
    public static let text = Color(red: 27.0 / 255.0, green: 26.0 / 255.0, blue: 22.0 / 255.0)
    /// Muted text — matches --text-muted (#6a6655).
    public static let textMuted = Color(red: 106.0 / 255.0, green: 102.0 / 255.0, blue: 85.0 / 255.0)
    /// Success — matches --success (#4f7a5a).
    public static let success = Color(red: 79.0 / 255.0, green: 122.0 / 255.0, blue: 90.0 / 255.0)
    /// Danger — matches --danger (#ef4444).
    public static let danger = Color(red: 239.0 / 255.0, green: 68.0 / 255.0, blue: 68.0 / 255.0)
}

/// Spacing scale matching `shared-web/tokens.css` (`--space-1` … `--space-6`).
public enum VirtueSpacing {
    public static let s1: CGFloat = 4
    public static let s2: CGFloat = 8
    public static let s3: CGFloat = 12
    public static let s4: CGFloat = 16
    public static let s5: CGFloat = 20
    public static let s6: CGFloat = 24
}

/// Corner radii matching `shared-web/tokens.css` conventions used by cards and buttons.
public enum VirtueRadius {
    public static let button: CGFloat = 6
    public static let card: CGFloat = 8
}
