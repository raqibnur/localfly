import Foundation

/// Every outside address the app links to, in one place.
enum Links {
    static let repository = URL(string: "https://github.com/raqibnur/localfly")!
    static let issues = URL(string: "https://github.com/raqibnur/localfly/issues")!
    static let sponsor = URL(string: "https://github.com/sponsors/raqibnur")!
    static let selfHosting = URL(string: "https://github.com/raqibnur/localfly/blob/main/docs/self-hosting.md")!
    static let website = URL(string: "https://raqibnur.github.io/localfly/")!
    /// Waitlist section of the website. `ref=app` tags sign-ups that came from inside the app.
    static let proWaitlist = URL(string: "https://raqibnur.github.io/localfly/?ref=app#pro")!
    static let latestRelease = URL(string: "https://api.github.com/repos/raqibnur/localfly/releases/latest")!
}
