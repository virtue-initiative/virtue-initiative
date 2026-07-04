import SwiftUI

/// A labeled group of `DetailRow`s, rendered as a `Section` in a `List`.
public struct StatusDetailSection: Identifiable {
    public let id = UUID()
    public let title: String
    public let rows: [(label: String, value: String)]

    public init(title: String, rows: [(label: String, value: String)]) {
        self.title = title
        self.rows = rows
    }
}

/// Dumb list of status sections/rows shared by the iOS and Mac status detail
/// screens. Each platform's coordinator supplies the data.
public struct StatusDetailList: View {
    private let sections: [StatusDetailSection]

    public init(sections: [StatusDetailSection]) {
        self.sections = sections
    }

    public var body: some View {
        List {
            ForEach(sections) { section in
                Section(section.title) {
                    ForEach(section.rows, id: \.label) { row in
                        DetailRow(label: row.label, value: row.value)
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(VirtueBrand.bg)
    }
}
