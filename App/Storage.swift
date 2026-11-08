import Foundation
import Security

/// A local app the user wants to expose.
struct SavedApp: Codable, Identifiable, Hashable {
    var id = UUID()
    var name: String
    var port: UInt16
    var startOnLaunch = false
    /// Stable 0…1 hue for the app's monogram.
    var hue: Double
    /// Public port to ask the relay for. Nil lets the relay pick a random one.
    var publicPort: UInt16?
    /// Address visitors use when a reverse proxy (e.g. Caddy) fronts the relay, like https://web.example.com.
    var publicAddress: String?

    init(name: String, port: UInt16, startOnLaunch: Bool = false) {
        self.name = name
        self.port = port
        self.startOnLaunch = startOnLaunch
        self.hue = Self.hue(for: name)
    }

    var initial: String { name.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() } ?? "•" }

    /// djb2, so the colour is the same on every launch (String.hashValue is randomized).
    static func hue(for name: String) -> Double {
        let hash = name.lowercased().utf8.reduce(UInt32(5381)) { ($0 << 5) &+ $0 &+ UInt32($1) }
        return Double(hash % 360) / 360
    }
}

/// Saved apps as JSON in Application Support.
enum AppStore {
    private static var url: URL {
        let folder = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Localfly", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder.appendingPathComponent("apps.json")
    }

    static func load() -> [SavedApp] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([SavedApp].self, from: data)) ?? []
    }

    static func save(_ apps: [SavedApp]) {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(apps) else { return }
        try? data.write(to: url, options: .atomic)
    }
}

/// The relay secret lives in the login keychain, not in preferences.
enum SecretStore {
    private static var query: [String: Any] { [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "com.raqibnur.localfly.relay-secret",
        kSecAttrAccount as String: "relay",
    ] }

    static func read() -> String {
        var lookup = query
        lookup[kSecReturnData as String] = true
        lookup[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(lookup as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    static func write(_ secret: String) {
        SecItemDelete(query as CFDictionary)
        guard !secret.isEmpty else { return }
        var item = query
        item[kSecValueData as String] = Data(secret.utf8)
        SecItemAdd(item as CFDictionary, nil)
    }
}
