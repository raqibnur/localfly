import CryptoKit
import Foundation

/// Wire protocol of bore (https://github.com/ekzhang/bore, MIT), version 0.6.
///
/// Every control message is a JSON value encoded the way `serde_json` encodes a Rust enum,
/// terminated by a single NUL byte. Unit variants are bare strings (`"Heartbeat"`), newtype
/// variants are single-key objects (`{"Hello":3000}`).
public enum BoreProtocol {
    /// Fixed control port every bore server listens on.
    public static let controlPort: UInt16 = 7835
    /// Maximum length of one control frame, excluding the delimiter.
    public static let maxFrameLength = 256

    public enum ServerMessage: Equatable, Sendable {
        case challenge(UUID)
        case hello(UInt16)
        case heartbeat
        case connection(UUID)
        case error(String)
    }

    public enum ProtocolError: Error, Equatable, LocalizedError {
        case frameTooLong
        case malformed(String)

        public var errorDescription: String? {
            switch self {
            case .frameTooLong: "Relay sent an oversized control frame"
            case .malformed(let detail): "Relay sent an unreadable message (\(detail))"
            }
        }
    }

    // MARK: Client → server

    public static func hello(port: UInt16) -> Data { frame(#"{"Hello":\#(port)}"#) }

    public static func accept(_ id: UUID) -> Data {
        frame(#"{"Accept":"\#(id.uuidString.lowercased())"}"#)
    }

    public static func authenticate(_ tag: String) -> Data { frame(#"{"Authenticate":"\#(tag)"}"#) }

    /// HMAC-SHA256 over the 16 raw challenge bytes, keyed with SHA256(secret), hex encoded.
    public static func answer(challenge: UUID, secret: String) -> String {
        let key = SymmetricKey(data: SHA256.hash(data: Data(secret.utf8)))
        let message = withUnsafeBytes(of: challenge.uuid) { Data($0) }
        let mac = HMAC<SHA256>.authenticationCode(for: message, using: key)
        return mac.map { String(format: "%02x", $0) }.joined()
    }

    // MARK: Server → client

    public static func decode(_ frame: Data) throws -> ServerMessage {
        let value: Any
        do {
            value = try JSONSerialization.jsonObject(with: frame, options: [.fragmentsAllowed])
        } catch {
            throw ProtocolError.malformed("invalid JSON")
        }
        if let unit = value as? String {
            guard unit == "Heartbeat" else { throw ProtocolError.malformed("unknown message \(unit)") }
            return .heartbeat
        }
        guard let object = value as? [String: Any], object.count == 1, let (key, payload) = object.first else {
            throw ProtocolError.malformed("unexpected shape")
        }
        switch key {
        case "Hello":
            guard let port = (payload as? NSNumber)?.intValue, let value = UInt16(exactly: port) else {
                throw ProtocolError.malformed("bad port")
            }
            return .hello(value)
        case "Challenge", "Connection":
            guard let text = payload as? String, let id = UUID(uuidString: text) else {
                throw ProtocolError.malformed("bad id")
            }
            return key == "Challenge" ? .challenge(id) : .connection(id)
        case "Error":
            return .error(payload as? String ?? "unknown error")
        default:
            throw ProtocolError.malformed("unknown message \(key)")
        }
    }

    private static func frame(_ json: String) -> Data {
        var data = Data(json.utf8)
        data.append(0)
        return data
    }
}

/// Splits a byte stream into NUL-delimited control frames.
public struct FrameReader: Sendable {
    private var buffer = Data()

    public init() {}

    /// Appends bytes and returns every complete frame now available.
    public mutating func push(_ bytes: Data) throws -> [Data] {
        buffer.append(bytes)
        var frames: [Data] = []
        while let end = buffer.firstIndex(of: 0) {
            let frame = buffer[buffer.startIndex..<end]
            guard frame.count <= BoreProtocol.maxFrameLength else { throw BoreProtocol.ProtocolError.frameTooLong }
            frames.append(Data(frame))
            buffer = Data(buffer[buffer.index(after: end)...])
        }
        guard buffer.count <= BoreProtocol.maxFrameLength else { throw BoreProtocol.ProtocolError.frameTooLong }
        return frames
    }

    /// Bytes received after the last complete frame.
    public mutating func drain() -> Data {
        defer { buffer = Data() }
        return buffer
    }
}
