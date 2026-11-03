import SwiftUI
#if os(macOS)
import AppKit
#endif

public struct VirtueButtonStyle: ButtonStyle {
    private let prominent: Bool
    private let large: Bool

    /// `large` is for the main actions on a screen: taller, semibold and as
    /// wide as the column it sits in.
    public init(prominent: Bool = false, large: Bool = false) {
        self.prominent = prominent
        self.large = large
    }

    public func makeBody(configuration: Configuration) -> some View {
        StyledButton(configuration: configuration, prominent: prominent, large: large)
    }

    // `@Environment` only updates inside a View, not a ButtonStyle, so the
    // disabled look lives here.
    private struct StyledButton: View {
        let configuration: Configuration
        let prominent: Bool
        let large: Bool
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .font(large ? .body.weight(.semibold) : .body)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 14)
                .padding(.vertical, large ? 10 : 7)
                .frame(maxWidth: large ? .infinity : nil)
                .background(
                    prominent
                        ? (configuration.isPressed ? VirtueBrand.accent.opacity(0.85) : VirtueBrand.accent)
                        : (configuration.isPressed ? VirtueBrand.border : VirtueBrand.bgSubtle)
                )
                .foregroundStyle(prominent ? Color.white : VirtueBrand.accent)
                .clipShape(RoundedRectangle(cornerRadius: large ? 8 : VirtueRadius.button, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: large ? 8 : VirtueRadius.button, style: .continuous)
                        .stroke(prominent ? Color.clear : VirtueBrand.border, lineWidth: 1)
                )
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1.0 : 0.45)
                .animation(.easeInOut(duration: 0.1), value: configuration.isPressed)
        }
    }
}
