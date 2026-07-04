import SwiftUI

public struct DetailRow: View {
    private let label: String
    private let value: String

    public init(label: String, value: String) {
        self.label = label
        self.value = value
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(VirtueBrand.textMuted)
            Text(value)
                .font(.body)
                .foregroundStyle(VirtueBrand.text)
        }
        .padding(.vertical, 2)
        .listRowBackground(VirtueBrand.surface)
    }
}
