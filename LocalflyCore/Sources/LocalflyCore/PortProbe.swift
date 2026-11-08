import Foundation
import Network

public enum PortProbe {
    /// Whether something accepts TCP connections on `host:port` (IPv4 or IPv6).
    public static func isListening(_ port: UInt16, host: String = "localhost", timeout: TimeInterval = 1) async -> Bool {
        await measure(host: host, port: port, timeout: timeout) != nil
    }

    /// TCP connect time to `host:port`, or nil if unreachable within `timeout`.
    public static func measure(host: String, port: UInt16, timeout: TimeInterval = 3) async -> TimeInterval? {
        let connection = NWConnection(to: BoreClient.endpoint(host, port), using: .tcp)
        let queue = DispatchQueue(label: "com.raqibnur.localfly.probe")
        let started = Date()
        return await withCheckedContinuation { continuation in
            let once = Once(continuation)
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: once.resume(Date().timeIntervalSince(started))
                case .waiting, .failed, .cancelled: once.resume(nil)
                default: return
                }
                connection.stateUpdateHandler = nil
                connection.cancel()
            }
            connection.start(queue: queue)
            queue.asyncAfter(deadline: .now() + timeout) {
                once.resume(nil)
                connection.cancel()
            }
        }
    }

    /// Resumes a continuation at most once; callers are serialized on one queue.
    private final class Once: @unchecked Sendable {
        private var continuation: CheckedContinuation<TimeInterval?, Never>?
        init(_ continuation: CheckedContinuation<TimeInterval?, Never>) { self.continuation = continuation }
        func resume(_ value: TimeInterval?) {
            continuation?.resume(returning: value)
            continuation = nil
        }
    }
}
