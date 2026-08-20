import Foundation
import TinyArcade

@main
private struct TinyArcadeReplaySmoke {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("replay smoke requires Paddle Guard .wasm")
        }
        let cartridge = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let makeRuntime: (Data) throws -> TinyArcadeRuntimeV1 = { bytes in
            try TinyArcadeRuntimeV1(privateCartridge: bytes) { config in
                config.max_memory_pages = 17
                config.max_steps = 500_000
                config.max_render_bytes = 20 * 1_024
                config.max_audio_bytes = 64
                config.max_state_bytes = 128
                config.rng_seed = 0x5041_4447
            }
        }

        let recorder = try makeRuntime(cartridge)
        try recorder.beginReplayRecording()
        expectFailure("duplicate replay begin") { try recorder.beginReplayRecording() }
        expectFailure("snapshot during replay") { _ = try recorder.suspend() }
        for (buttons, clock) in [
            (UInt32(1 << 4), UInt32(0)),
            (UInt32(0), UInt32(0)),
            (UInt32(1 << 0), UInt32(16)),
            (UInt32(1 << 1), UInt32(32)),
        ] {
            _ = try recorder.tickMedia(buttons: buttons, clockMilliseconds: clock)
        }
        let replay = try recorder.finishReplayRecording()
        precondition(replay.count == 529)
        precondition(replay.prefix(4) == Data("TAR1".utf8))
        expectFailure("duplicate replay finish") { _ = try recorder.finishReplayRecording() }
        try recorder.close()

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-replay-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let replayURL = directory.appendingPathComponent("paddle-guard-0.1.0.tareplay")
        try replay.write(to: replayURL, options: .atomic)
        let loaded = try Data(contentsOf: replayURL, options: .mappedIfSafe)
        precondition(loaded == replay)

        let verifier = try makeRuntime(cartridge)
        let verifiedSteps = try verifier.verifyReplay(loaded)
        precondition(verifiedSteps == 4)
        try verifier.close()

        var tampered = loaded
        tampered[tampered.index(before: tampered.endIndex)] ^= 0xff
        let tamperVerifier = try makeRuntime(cartridge)
        expectFailure("changed replay digest") { try tamperVerifier.verifyReplay(tampered) }
        try tamperVerifier.close()

        var sameManifestDifferentBytes = cartridge
        sameManifestDifferentBytes.append(contentsOf: [0x00, 0x01, 0x00])
        let wrongCartridge = try makeRuntime(sameManifestDifferentBytes)
        expectFailure("same manifest different cartridge") {
            try wrongCartridge.verifyReplay(loaded)
        }
        try wrongCartridge.close()

        let reproduction = try makeRuntime(cartridge)
        try reproduction.beginReplayRecording()
        for (buttons, clock) in [
            (UInt32(1 << 4), UInt32(0)),
            (UInt32(0), UInt32(0)),
            (UInt32(1 << 0), UInt32(16)),
            (UInt32(1 << 1), UInt32(32)),
        ] {
            _ = try reproduction.tickMedia(buttons: buttons, clockMilliseconds: clock)
        }
        let reproducedReplay = try reproduction.finishReplayRecording()
        precondition(reproducedReplay == replay)
        try reproduction.cancelReplayRecording()
        expectFailure("copy after cancellation") { _ = try reproduction.finishReplayRecording() }
        try reproduction.close()

        print("OK: iOS record → atomic file exchange → exact replay → tamper/hash refusal")
    }

    @MainActor
    private static func expectFailure(_ context: String, _ body: () throws -> Void) {
        do {
            try body()
            preconditionFailure("expected failure: \(context)")
        } catch let error as TinyArcadeRuntimeError {
            precondition(!error.message.isEmpty, "missing diagnostic: \(context)")
        } catch {
            preconditionFailure("unexpected error for \(context): \(error)")
        }
    }
}
