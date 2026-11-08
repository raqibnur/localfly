import Foundation
import Network
import Testing
@testable import LocalflyCore

/// Runs against a real `bore server` on 127.0.0.1. Set BORE_BIN or install with `cargo install bore-cli`.
/// The control port (7835) is fixed, so these tests run one at a time.
@Suite(.serialized, .enabled(if: BoreServer.binary != nil, "bore binary not found"))
@MainActor
struct TunnelManagerTests {
    @Test func startProxiesTrafficAndStopClosesIt() async throws {
        let server = try await BoreServer.launch()
        defer { server.stop() }
        let app = try await EchoServer.start()
        defer { app.stop() }

        let manager = TunnelManager()
        let info = await manager.start(localPort: app.port, options: TunnelOptions(relay: "127.0.0.1"))
        #expect(info.status == .live)
        let remotePort = try #require(info.remotePort)
        #expect(info.publicURL == "127.0.0.1:\(remotePort)")
        #expect(manager.list().map(\.id) == [info.id])

        let reply = try await roundTrip(port: remotePort, payload: "ping over the tunnel")
        #expect(reply == "ping over the tunnel")

        manager.stop(info.id)
        #expect(manager.list().isEmpty)
        // bore notices the closed control connection on its next heartbeat (every 500 ms).
        let closed = try await eventually(within: 1.5) { await !PortProbe.isListening(remotePort, host: "127.0.0.1", timeout: 0.3) }
        #expect(closed)
    }

    @Test func publishesStatusChanges() async throws {
        let server = try await BoreServer.launch()
        defer { server.stop() }
        let manager = TunnelManager()
        var updates = manager.updates().makeAsyncIterator()

        let info = await manager.start(localPort: 9, options: TunnelOptions(relay: "127.0.0.1"))
        manager.stop(info.id)

        var seen: [TunnelStatus] = []
        while seen.last != .stopped, let next = await updates.next() { seen.append(next.status) }
        #expect(seen == [.connecting, .live, .stopped])
    }

    @Test func authenticatesWithSecret() async throws {
        let server = try await BoreServer.launch(secret: "hunter2")
        defer { server.stop() }
        let app = try await EchoServer.start()
        defer { app.stop() }
        let manager = TunnelManager()

        let good = await manager.start(localPort: app.port, options: TunnelOptions(relay: "127.0.0.1", secret: "hunter2"))
        #expect(good.status == .live)
        #expect(try await roundTrip(port: try #require(good.remotePort), payload: "authed") == "authed")

        let missing = await manager.start(localPort: app.port, options: TunnelOptions(relay: "127.0.0.1"))
        #expect(missing.status == .error("This relay requires a secret"))

        let wrong = await manager.start(localPort: app.port, options: TunnelOptions(relay: "127.0.0.1", secret: "nope"))
        guard case .error = wrong.status else { Issue.record("wrong secret went \(wrong.status)"); return }
        manager.stopAll()
    }

    @Test func requestsFixedPublicPort() async throws {
        let server = try await BoreServer.launch()
        defer { server.stop() }
        let app = try await EchoServer.start()
        defer { app.stop() }
        let manager = TunnelManager()

        let fixed = await manager.start(localPort: app.port, remotePort: 40123, options: TunnelOptions(relay: "127.0.0.1"))
        #expect(fixed.status == .live)
        #expect(fixed.remotePort == 40123)
        #expect(try await roundTrip(port: 40123, payload: "fixed") == "fixed")

        // A second tunnel can't take a port that is in use.
        let clash = await manager.start(localPort: app.port, remotePort: 40123, options: TunnelOptions(relay: "127.0.0.1"))
        guard case .error(let reason) = clash.status else { Issue.record("expected error, got \(clash.status)"); return }
        #expect(reason.hasPrefix("Relay refused the tunnel"))
        manager.stopAll()
    }

    @Test func reportsUnreachableRelay() async {
        let manager = TunnelManager()
        let info = await manager.start(localPort: 3000, options: TunnelOptions(relay: "127.0.0.1:1"))
        guard case .error(let reason) = info.status else { Issue.record("expected error, got \(info.status)"); return }
        #expect(reason.contains("Couldn’t reach relay"))
    }

    @Test func autoReconnectKeepsTryingAndReturnsLive() async throws {
        let manager = TunnelManager()
        var updates = manager.updates().makeAsyncIterator()
        let first = await manager.start(localPort: 9, options: TunnelOptions(relay: "127.0.0.1", autoReconnect: true))
        guard case .error = first.status else { Issue.record("relay should be down, got \(first.status)"); return }
        #expect(manager.info(first.id)?.status == .connecting)

        let server = try await BoreServer.launch()
        defer { server.stop() }
        while let next = await updates.next(), next.status != .live {}
        #expect(manager.info(first.id)?.status == .live)
        manager.stop(first.id)
    }
}

// MARK: Helpers

private func roundTrip(port: UInt16, payload: String) async throws -> String {
    let connection = NWConnection(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!, using: .tcp)
    defer { connection.cancel() }
    connection.start(queue: .global())
    return try await withCheckedThrowingContinuation { continuation in
        connection.send(content: Data(payload.utf8), completion: .contentProcessed { error in
            if let error { return continuation.resume(throwing: error) }
            connection.receive(minimumIncompleteLength: payload.utf8.count, maximumLength: 4096) { data, _, _, error in
                if let error { return continuation.resume(throwing: error) }
                continuation.resume(returning: String(decoding: data ?? Data(), as: UTF8.self))
            }
        })
    }
}

private func eventually(within seconds: TimeInterval, _ check: @Sendable () async -> Bool) async throws -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if await check() { return true }
        try await Task.sleep(nanoseconds: 100_000_000)
    }
    return false
}

final class BoreServer: @unchecked Sendable {
    static var binary: String? {
        let candidates = [ProcessInfo.processInfo.environment["BORE_BIN"], NSHomeDirectory() + "/.cargo/bin/bore", "/opt/homebrew/bin/bore", "/usr/local/bin/bore"]
        return candidates.compactMap { $0 }.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    private let process = Process()

    static func launch(secret: String? = nil) async throws -> BoreServer {
        let server = BoreServer()
        server.process.executableURL = URL(fileURLWithPath: binary!)
        server.process.arguments = ["server", "--bind-addr", "127.0.0.1", "--bind-tunnels", "127.0.0.1"] + (secret.map { ["--secret", $0] } ?? [])
        server.process.standardOutput = FileHandle.nullDevice
        server.process.standardError = FileHandle.nullDevice
        try server.process.run()
        guard try await eventually(within: 5, { await PortProbe.isListening(7835, host: "127.0.0.1", timeout: 0.3) }) else {
            server.stop()
            throw CocoaError(.featureUnsupported, userInfo: [NSLocalizedDescriptionKey: "bore server did not start"])
        }
        return server
    }

    func stop() {
        guard process.isRunning else { return }
        process.terminate()
        process.waitUntilExit()
    }
}

/// Minimal TCP echo server standing in for a local dev app.
final class EchoServer: @unchecked Sendable {
    private let listener: NWListener
    let port: UInt16

    private init(listener: NWListener, port: UInt16) {
        self.listener = listener
        self.port = port
    }

    static func start() async throws -> EchoServer {
        let listener = try NWListener(using: .tcp, on: .any)
        listener.newConnectionHandler = { connection in
            connection.start(queue: .global())
            echo(connection)
        }
        let port: UInt16 = try await withCheckedThrowingContinuation { continuation in
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready: continuation.resume(returning: listener.port!.rawValue)
                case .failed(let error): continuation.resume(throwing: error)
                default: return
                }
                listener.stateUpdateHandler = nil
            }
            listener.start(queue: .global())
        }
        return EchoServer(listener: listener, port: port)
    }

    private static func echo(_ connection: NWConnection) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, isComplete, error in
            if let data, !data.isEmpty { connection.send(content: data, completion: .idempotent) }
            if isComplete || error != nil { return connection.cancel() }
            echo(connection)
        }
    }

    func stop() { listener.cancel() }
}
