import Foundation
import Testing
@testable import LocalflyCore

@Suite struct BoreProtocolTests {
    @Test func encodesClientMessagesLikeSerde() {
        #expect(BoreProtocol.hello(port: 0) == Data(#"{"Hello":0}"#.utf8) + [0])
        let id = UUID(uuidString: "67E55044-10B1-426F-9247-BB680E5FE0C8")!
        #expect(BoreProtocol.accept(id) == Data(#"{"Accept":"67e55044-10b1-426f-9247-bb680e5fe0c8"}"#.utf8) + [0])
    }

    @Test func decodesServerMessages() throws {
        let id = "67e55044-10b1-426f-9247-bb680e5fe0c8"
        #expect(try BoreProtocol.decode(Data(#""Heartbeat""#.utf8)) == .heartbeat)
        #expect(try BoreProtocol.decode(Data(#"{"Hello":41234}"#.utf8)) == .hello(41234))
        #expect(try BoreProtocol.decode(Data(#"{"Connection":"\#(id)"}"#.utf8)) == .connection(UUID(uuidString: id)!))
        #expect(try BoreProtocol.decode(Data(#"{"Challenge":"\#(id)"}"#.utf8)) == .challenge(UUID(uuidString: id)!))
        #expect(try BoreProtocol.decode(Data(#"{"Error":"port already in use"}"#.utf8)) == .error("port already in use"))
        #expect(throws: BoreProtocol.ProtocolError.self) { try BoreProtocol.decode(Data(#"{"Hello":70000}"#.utf8)) }
    }

    /// Reference value from Python: hmac.new(sha256(b"s3cret").digest(), uuid.bytes, sha256).hexdigest()
    @Test func answersChallengeWithHmacOfRawUUIDBytes() {
        let challenge = UUID(uuidString: "67e55044-10b1-426f-9247-bb680e5fe0c8")!
        #expect(BoreProtocol.answer(challenge: challenge, secret: "s3cret") == "9d012242868ec7ae18e13fcc858d7d8a7003339a47723aab17aee26a30ad4723")
    }

    @Test func splitsFramesAcrossChunks() throws {
        var reader = FrameReader()
        #expect(try reader.push(Data(#""Heart"#.utf8)).isEmpty)
        let frames = try reader.push(Data(#"beat""#.utf8) + [0] + Data(#"{"Hello":1}"#.utf8) + [0] + Data("raw".utf8))
        #expect(frames == [Data(#""Heartbeat""#.utf8), Data(#"{"Hello":1}"#.utf8)])
        #expect(reader.drain() == Data("raw".utf8))
    }

    @Test func rejectsOversizedFrames() {
        var reader = FrameReader()
        #expect(throws: BoreProtocol.ProtocolError.frameTooLong) {
            try reader.push(Data(repeating: 65, count: BoreProtocol.maxFrameLength + 1))
        }
    }

    @Test func parsesRelayEndpoint() {
        #expect(TunnelOptions(relay: "bore.pub").relayEndpoint == ("bore.pub", 7835))
        #expect(TunnelOptions(relay: " relay.example.com:9000 ").relayEndpoint == ("relay.example.com", 9000))
        #expect(TunnelOptions(relay: "").relayEndpoint == ("bore.pub", 7835))
    }
}
