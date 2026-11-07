import SwiftUI

public struct Card<Content: View>: View {
    private let content: Content
    private let cornerRadius: CGFloat

    /// `cornerRadius` defaults to `VirtueRadius.card`. iOS passes `VirtueRadius.cardLarge`.
    public init(cornerRadius: CGFloat = VirtueRadius.card, @ViewBuilder content: () -> Content) {
        self.cornerRadius = cornerRadius
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .padding(VirtueSpacing.s5)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(VirtueBrand.surface)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .stroke(VirtueBrand.border, lineWidth: 1)
        )
    }
}
