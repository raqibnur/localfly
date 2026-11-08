import Foundation
import Network

/// Traffic counters for one tunnel.
public struct TunnelStats: Equatable, Sendable {
    /// Bytes received from visitors and forwarded to the local app.
    public var bytesIn: UInt64 = 0
    /// Bytes sent from the local app back to visitors.
    public var bytesOut: UInt64 = 0
    public var activeConnections = 0
    public var totalConnections = 0

    public init() {}
}

/// A single bore client session: one control connection plus one proxied pair per visitor.
///
/// All state is confined to a private serial queue. Events are delivered on that queue.
final class BoreClient: @unchecked Sendable {
    enum Event: Sendable {
        case live(remotePort: UInt16)
        case failed(String)
        case stats(TunnelStats)
    }

    struct Config: Sendable {
        var localHost: String
        var localPort: UInt16
        var relayHost: String
        var relayPort: UInt16
        var secret: String?
        var requestedPort: UInt16
    }

    private enum Phase { case idle, connecting, awaitingChallenge, awaitingHello, live, closed }

    /// How long the handshake may take before giving up.
    static let handshakeTimeout: TimeInterval = 6
    /// The server sends a heartbeat every 500 ms; silence this long means the relay is gone.
    static let silenceTimeout: TimeInterval = 6

    private let config: Config
    private let queue = DispatchQueue(label: "com.raqibnur.localfly.bore-client")
    private let onEvent: @Sendable (Event) -> Void

    private var phase = Phase.idle
    private var control: NWConnection?
    private var reader = FrameReader()
    private var proxies: [ObjectIdentifier: ProxyPair] = [:]
    private var timer: DispatchSourceTimer?
    private var startedAt = Date()
    private var lastMessageAt = Date()
    private var stats = TunnelStats()
    private var reportedStats = TunnelStats()

    init(config: Config, onEvent: @escaping @Sendable (Event) -> Void) {
        self.config = config
        self.onEvent = onEvent
    }

    func start() {
        queue.async { self.connect() }
    }

    /// Tears down every connection. No further events are delivered.
    func cancel() {
        queue.async { self.teardown() }
    }

    // MARK: Control connection

    private func connect() {
        guard phase == .idle else { return }
        phase = .connecting
        startedAt = Date()

        let connection = NWConnection(to: Self.endpoint(config.relayHost, config.relayPort), using: .tcp)
        control = connection
        connection.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.handshakeStarted()
            case .waiting(let error), .failed(let error):
                self.fail("Couldn’t reach relay \(self.config.relayHost) (\(error.localizedDescription))")
            default:
                break
            }
        }
        connection.start(queue: queue)

        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + 0.5, repeating: 0.5)
        timer.setEventHandler { [weak self] in self?.tick() }
        timer.resume()
        self.timer = timer
    }

    private func handshakeStarted() {
        guard phase == .connecting, let control else { return }
        lastMessageAt = Date()
        if config.secret != nil {
            phase = .awaitingChallenge
        } else {
            phase = .awaitingHello
            control.send(content: BoreProtocol.hello(port: config.requestedPort), completion: .idempotent)
        }
        receiveControl()
    }

    private func receiveControl() {
        control?.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] data, _, isComplete, error in
            guard let self, self.phase != .closed else { return }
            if let data, !data.isEmpty {
                do {
                    for frame in try self.reader.push(data) {
                        try self.handle(BoreProtocol.decode(frame))
                        if self.phase == .closed { return }
                    }
                } catch {
                    return self.fail(error.localizedDescription)
                }
            }
            if let error {
                return self.fail("Relay connection dropped (\(error.localizedDescription))")
            }
            if isComplete {
                return self.fail(self.phase == .live ? "Relay closed the tunnel" : "Relay closed the connection during setup")
            }
            self.receiveControl()
        }
    }

    private func handle(_ message: BoreProtocol.ServerMessage) throws {
        lastMessageAt = Date()
        switch (phase, message) {
        case (.awaitingChallenge, .challenge(let challenge)):
            guard let secret = config.secret else { return }
            control?.send(content: BoreProtocol.authenticate(BoreProtocol.answer(challenge: challenge, secret: secret)), completion: .idempotent)
            control?.send(content: BoreProtocol.hello(port: config.requestedPort), completion: .idempotent)
            phase = .awaitingHello
        case (.awaitingChallenge, .error(let message)):
            fail("Relay refused the tunnel: \(message)")
        case (.awaitingChallenge, _):
            fail("A secret is set, but this relay doesn’t use one")
        case (.awaitingHello, .hello(let port)):
            phase = .live
            onEvent(.live(remotePort: port))
        case (.awaitingHello, .challenge):
            fail("This relay requires a secret")
        case (.awaitingHello, .error(let message)):
            fail("Relay refused the tunnel: \(message)")
        case (.awaitingHello, _):
            fail("Relay sent an unexpected reply")
        case (.live, .connection(let id)):
            openProxy(id)
        case (.live, _):
            break // Heartbeats, and messages bore itself only logs.
        default:
            break
        }
    }

    private func tick() {
        let now = Date()
        switch phase {
        case .connecting, .awaitingChallenge, .awaitingHello:
            guard now.timeIntervalSince(startedAt) > Self.handshakeTimeout else { break }
            // A relay without a secret never sends a challenge, so the wait just runs out.
            fail(phase == .awaitingChallenge
                ? "Relay didn’t ask for a secret. Clear it in Relay settings."
                : "Timed out connecting to relay \(config.relayHost)")
        case .live:
            if now.timeIntervalSince(lastMessageAt) > Self.silenceTimeout {
                return fail("Lost contact with relay \(config.relayHost)")
            }
            if stats != reportedStats {
                reportedStats = stats
                onEvent(.stats(stats))
            }
        case .idle, .closed:
            break
        }
    }

    private func fail(_ reason: String) {
        guard phase != .closed else { return }
        teardown()
        onEvent(.failed(reason))
    }

    private func teardown() {
        phase = .closed
        timer?.cancel()
        timer = nil
        control?.stateUpdateHandler = nil
        control?.cancel()
        control = nil
        for proxy in proxies.values { proxy.cancel() }
        proxies.removeAll()
    }

    // MARK: Visitor connections

    private func openProxy(_ id: UUID) {
        let proxy = ProxyPair(
            id: id,
            relay: Self.endpoint(config.relayHost, config.relayPort),
            local: Self.endpoint(config.localHost, config.localPort),
            secret: config.secret,
            queue: queue
        )
        let key = ObjectIdentifier(proxy)
        proxies[key] = proxy
        stats.activeConnections += 1
        stats.totalConnections += 1
        proxy.onTraffic = { [weak self] inbound, outbound in
            self?.stats.bytesIn += inbound
            self?.stats.bytesOut += outbound
        }
        proxy.onClose = { [weak self] in
            guard let self, self.proxies.removeValue(forKey: key) != nil else { return }
            self.stats.activeConnections -= 1
        }
        proxy.start()
    }

    static func endpoint(_ host: String, _ port: UInt16) -> NWEndpoint {
        .hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: port) ?? .any)
    }
}

/// Pairs one relay data connection with one connection to the local app and pumps bytes both ways.
private final class ProxyPair: @unchecked Sendable {
    private let id: UUID
    private let secret: String?
    private let queue: DispatchQueue
    private let remote: NWConnection
    private let local: NWConnection
    private var reader = FrameReader()
    private var closed = false
    private var halfClosed = 0

    var onTraffic: ((_ inbound: UInt64, _ outbound: UInt64) -> Void)?
    var onClose: (() -> Void)?

    init(id: UUID, relay: NWEndpoint, local: NWEndpoint, secret: String?, queue: DispatchQueue) {
        self.id = id
        self.secret = secret
        self.queue = queue
        self.remote = NWConnection(to: relay, using: .tcp)
        self.local = NWConnection(to: local, using: .tcp)
    }

    func start() {
        remote.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready: self.secret == nil ? self.accept() : self.awaitChallenge()
            case .failed, .waiting: self.cancel()
            default: break
            }
        }
        remote.start(queue: queue)
        queue.asyncAfter(deadline: .now() + 6) { [weak self] in
            if let self, !self.closed, self.local.state != .ready { self.cancel() }
        }
    }

    private func awaitChallenge() {
        remote.receive(minimumIncompleteLength: 1, maximumLength: BoreProtocol.maxFrameLength + 1) { [weak self] data, _, isComplete, error in
            guard let self, !self.closed else { return }
            guard let data, error == nil, !isComplete,
                  let frames = try? self.reader.push(data) else { return self.cancel() }
            guard let first = frames.first else { return self.awaitChallenge() }
            guard case .challenge(let challenge)? = try? BoreProtocol.decode(first), let secret = self.secret else {
                return self.cancel()
            }
            self.remote.send(content: BoreProtocol.authenticate(BoreProtocol.answer(challenge: challenge, secret: secret)), completion: .idempotent)
            self.accept()
        }
    }

    private func accept() {
        remote.send(content: BoreProtocol.accept(id), completion: .idempotent)
        let leftover = reader.drain()
        local.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                if !leftover.isEmpty { self.local.send(content: leftover, completion: .idempotent) }
                self.pump(from: self.remote, to: self.local, inbound: true)
                self.pump(from: self.local, to: self.remote, inbound: false)
            case .failed, .waiting:
                self.cancel()
            default:
                break
            }
        }
        local.start(queue: queue)
    }

    private func pump(from source: NWConnection, to sink: NWConnection, inbound: Bool) {
        source.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, isComplete, error in
            guard let self, !self.closed else { return }
            if let data, !data.isEmpty {
                let count = UInt64(data.count)
                self.onTraffic?(inbound ? count : 0, inbound ? 0 : count)
                sink.send(content: data, completion: .contentProcessed { [weak self] sendError in
                    guard let self, !self.closed else { return }
                    if sendError != nil { return self.cancel() }
                    if isComplete { return self.finish(sink) }
                    self.pump(from: source, to: sink, inbound: inbound)
                })
                return
            }
            if error != nil { return self.cancel() }
            if isComplete { return self.finish(sink) }
            self.pump(from: source, to: sink, inbound: inbound)
        }
    }

    /// Propagates a half-close; once both directions have ended, the pair is released.
    private func finish(_ sink: NWConnection) {
        sink.send(content: nil, contentContext: .finalMessage, isComplete: true, completion: .idempotent)
        halfClosed += 1
        if halfClosed == 2 { cancel() }
    }

    func cancel() {
        guard !closed else { return }
        closed = true
        remote.stateUpdateHandler = nil
        local.stateUpdateHandler = nil
        remote.cancel()
        local.cancel()
        onClose?()
        onTraffic = nil
        onClose = nil
    }
}
