import SwiftUI
#if os(macOS)
import AppKit
#endif

public extension View {
    /// Shows the pointing-hand cursor while the pointer is over this view, so
    /// a click target reads as one. Does nothing on iOS.
    func pointingHandCursor(_ enabled: Bool = true) -> some View {
        modifier(PointingHandCursor(enabled: enabled))
    }
}

private struct PointingHandCursor: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        #if os(macOS)
        // `set()` rather than push/pop: AppKit can skip a pop (a view removed
        // mid-hover, a sheet opening), which left the hand stuck before.
        content.onContinuousHover { phase in
            switch phase {
            case .active:
                (enabled ? NSCursor.pointingHand : NSCursor.arrow).set()
            case .ended:
                NSCursor.arrow.set()
            }
        }
        #else
        content
        #endif
    }
}
