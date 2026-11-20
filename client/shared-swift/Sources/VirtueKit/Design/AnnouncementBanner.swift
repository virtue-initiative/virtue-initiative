import SwiftUI

/// The CORE-010 announcement, shown below an app's main actions (CLIENT-033).
/// `info` reads as a small link; `warning` and `urgent` sit on an orange or red
/// background. With a url, tapping or clicking it opens the url.
public struct AnnouncementBanner: View {
    private let announcement: CoreAnnouncement
    @Environment(\.openURL) private var openURL

    public init(_ announcement: CoreAnnouncement) {
        self.announcement = announcement
    }

    private var url: URL? {
        announcement.url.flatMap(URL.init(string:))
    }

    public var body: some View {
        if let url {
            Button {
                openURL(url)
            } label: {
                content
            }
            .buttonStyle(.plain)
            .pointingHandCursor()
            .accessibilityAddTraits(.isLink)
        } else {
            content
        }
    }

    @ViewBuilder
    private var content: some View {
        switch announcement.level {
        case .info:
            Text(announcement.message)
                .font(.footnote)
                .foregroundColor(url == nil ? VirtueBrand.textMuted : VirtueBrand.link)
                .underline(url != nil)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity)
        case .warning, .urgent:
            Text(announcement.message)
                .font(.callout.weight(.medium))
                .foregroundColor(.white)
                .underline(url != nil)
                .multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, VirtueSpacing.s4)
                .padding(.vertical, VirtueSpacing.s3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    announcement.level == .urgent
                        ? VirtueBrand.announcementUrgent : VirtueBrand.announcementWarning
                )
                .clipShape(RoundedRectangle(cornerRadius: VirtueRadius.button, style: .continuous))
        }
    }
}
