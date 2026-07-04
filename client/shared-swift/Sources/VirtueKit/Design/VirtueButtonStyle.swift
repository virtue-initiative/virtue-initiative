import SwiftUI

public struct VirtueButtonStyle: ButtonStyle {
    private let prominent: Bool

    public init(prominent: Bool = false) {
        self.prominent = prominent
    }

    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(
                prominent
                    ? (configuration.isPressed ? VirtueBrand.accent.opacity(0.85) : VirtueBrand.accent)
                    : (configuration.isPressed ? VirtueBrand.border : VirtueBrand.bgSubtle)
            )
            .foregroundStyle(prominent ? Color.white : VirtueBrand.accent)
            .clipShape(RoundedRectangle(cornerRadius: VirtueRadius.button, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: VirtueRadius.button, style: .continuous)
                    .stroke(prominent ? Color.clear : VirtueBrand.border, lineWidth: 1)
            )
            .animation(.easeInOut(duration: 0.1), value: configuration.isPressed)
    }
}
