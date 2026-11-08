import Foundation

/// Looks for a newer GitHub release. Read-only: it only links to the download, never installs.
enum UpdateChecker {
    struct Release: Equatable {
        let version: String
        let page: URL
    }

    private struct Payload: Decodable {
        let tag_name: String
        let html_url: URL
        let draft: Bool
        let prerelease: Bool
    }

    static var currentVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0"
    }

    /// The latest release if it is newer than this build, otherwise nil. Network failures count as "nothing new".
    static func check() async -> Release? {
        var request = URLRequest(url: Links.latestRelease, timeoutInterval: 15)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        guard let (data, response) = try? await URLSession.shared.data(for: request),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let payload = try? JSONDecoder().decode(Payload.self, from: data),
              !payload.draft, !payload.prerelease else { return nil }
        let version = payload.tag_name.trimmingCharacters(in: CharacterSet(charactersIn: "vV"))
        return isNewer(version, than: currentVersion) ? Release(version: version, page: payload.html_url) : nil
    }

    /// Numeric comparison of dotted versions: 0.10.0 > 0.9.2.
    static func isNewer(_ candidate: String, than current: String) -> Bool {
        let a = candidate.split(separator: ".").map { Int($0) ?? 0 }
        let b = current.split(separator: ".").map { Int($0) ?? 0 }
        for index in 0..<max(a.count, b.count) {
            let left = index < a.count ? a[index] : 0
            let right = index < b.count ? b[index] : 0
            if left != right { return left > right }
        }
        return false
    }
}
