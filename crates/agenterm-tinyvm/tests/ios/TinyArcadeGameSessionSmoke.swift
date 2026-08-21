import Foundation
import TinyArcade

@main
private struct TinyArcadeGameSessionSmoke {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("game-session smoke requires Paddle Guard .wasm")
        }
        let cartridge = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let makeRuntime: () throws -> TinyArcadeRuntimeV1 = {
            try TinyArcadeRuntimeV1(privateCartridge: cartridge) { config in
                config.max_memory_pages = 17
                config.max_steps = 500_000
                config.max_render_bytes = 20 * 1_024
                config.max_audio_bytes = 64
                config.max_state_bytes = 128
            }
        }
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-game-session-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = try TinyArcadeSnapshotStoreV1(
            directoryURL: directory,
            maximumSnapshotBytes: 1_024
        )

        let fresh = try store.openSession(makeRuntime: makeRuntime)
        let session = try TinyArcadeGameSessionV1(restored: fresh)
        try session.setButtons(.primary, forSource: 1)
        try session.setButtons(.right, forSource: 2)
        precondition(session.input.buttons == [.primary, .right])
        let launch = try session.tick(elapsedMilliseconds: 0)
        guard case .indexed2D = launch.renderFrame else {
            preconditionFailure("Paddle Guard must render indexed2d")
        }
        try session.setButtons([], forSource: 1)
        precondition(session.input.buttons == .right)
        _ = try session.tick(elapsedMilliseconds: 16)
        precondition(session.gameClockMilliseconds == 16)
        try session.save(to: store)
        try session.close()
        expectSessionFailure(.closed, "closed session") {
            _ = try session.tick(elapsedMilliseconds: 0)
        }

        let restored = try store.openSession(makeRuntime: makeRuntime)
        precondition(restored.disposition == .restored)
        precondition(restored.gameClockMilliseconds == 16)
        let resumed = try TinyArcadeGameSessionV1(restored: restored)
        let invalidButtons = TinyArcadeButtonsV1(rawValue: 1 << 31)
        expectInputFailure(.unknownButtons, "unknown input bit") {
            try resumed.setButtons(invalidButtons, forSource: 9)
        }
        for index in 0..<TinyArcadeInputStateV1.maximumSourceCount {
            try resumed.setButtons(.left, forSource: UInt64(100 + index))
        }
        expectInputFailure(.tooManySources, "input source ceiling") {
            try resumed.setButtons(.right, forSource: 1_000)
        }
        resumed.releaseAllInputs()
        precondition(resumed.input.buttons.isEmpty)
        expectSessionFailure(.frameAdvanceTooLarge, "background-sized frame delta") {
            _ = try resumed.tick(elapsedMilliseconds: 251)
        }
        precondition(resumed.gameClockMilliseconds == 16)
        try resumed.setButtons(.right, forSource: 2)
        _ = try resumed.tick(elapsedMilliseconds: 16)
        precondition(resumed.gameClockMilliseconds == 32)
        try resumed.save(to: store)
        try resumed.close()

        let direct = try makeRuntime()
        _ = try direct.tickMedia(buttons: TinyArcadeButtonsV1.primary.rawValue, clockMilliseconds: 100)
        for (buttons, clock) in [(UInt32(1 << 31), UInt32(101)), (UInt32(0), UInt32(99))] {
            do {
                _ = try direct.tickMedia(buttons: buttons, clockMilliseconds: clock)
                preconditionFailure("invalid direct host input must fail")
            } catch let error as TinyArcadeRuntimeError {
                precondition(error.status == Int32(TINYARCADE_INVALID_ARGUMENT.rawValue))
            }
        }
        _ = try direct.tickMedia(buttons: 0, clockMilliseconds: 100)
        try direct.close()

        let exhausted = try TinyArcadeGameSessionV1(
            runtime: makeRuntime(),
            gameClockMilliseconds: UInt32.max
        )
        expectSessionFailure(.clockExhausted, "game clock exhaustion") {
            _ = try exhausted.tick(elapsedMilliseconds: 1)
        }
        try exhausted.close()

        let finalRestore = try store.openSession(makeRuntime: makeRuntime)
        precondition(finalRestore.disposition == .restored)
        precondition(finalRestore.gameClockMilliseconds == 32)
        try finalRestore.runtime.close()

        print("OK: multi-source input → bounded monotonic ticks → snapshot clock restore → invalid host input recovery")
    }

    @MainActor
    private static func expectInputFailure(
        _ expected: TinyArcadeInputStateError,
        _ context: String,
        _ body: () throws -> Void
    ) {
        do {
            try body()
            preconditionFailure("expected \(context) failure")
        } catch let error as TinyArcadeInputStateError {
            precondition(error == expected)
        } catch {
            preconditionFailure("unexpected \(context) error: \(error)")
        }
    }

    @MainActor
    private static func expectSessionFailure(
        _ expected: TinyArcadeGameSessionError,
        _ context: String,
        _ body: () throws -> Void
    ) {
        do {
            try body()
            preconditionFailure("expected \(context) failure")
        } catch let error as TinyArcadeGameSessionError {
            precondition(error == expected)
        } catch {
            preconditionFailure("unexpected \(context) error: \(error)")
        }
    }
}
