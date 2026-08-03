import SwiftUI

public struct Card<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .padding(VirtueSpacing.s5)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(VirtueBrand.surface)
        .clipShape(RoundedRectangle(cornerRadius: VirtueRadius.card, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: VirtueRadius.card, style: .continuous)
                .stroke(VirtueBrand.border, lineWidth: 1)
        )
    }
}
