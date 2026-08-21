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
            try TinyArcadeRuntimeV1(
                privateCartridge: cartridge,
                distributionPolicy: .sdkTestExternalCartridges
            ) { config in
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

        expectPacerFailure(.invalidMaximumFrameAdvance, "invalid pacer ceiling") {
            _ = try TinyArcadeFramePacerV1(maximumFrameAdvanceMilliseconds: 0)
        }
        var pacer = try TinyArcadeFramePacerV1()
        let firstPacedDelta = try pacer.elapsedMilliseconds(at: 10)
        let secondPacedDelta = try pacer.elapsedMilliseconds(at: 10.015625)
        let thirdPacedDelta = try pacer.elapsedMilliseconds(at: 10.03125)
        precondition(firstPacedDelta == 0)
        precondition(secondPacedDelta == 15)
        precondition(thirdPacedDelta == 16)
        expectPacerFailure(.timestampWentBackwards, "backwards frame timestamp") {
            _ = try pacer.elapsedMilliseconds(at: 10.02)
        }
        expectPacerFailure(.invalidTimestamp, "non-finite frame timestamp") {
            _ = try pacer.elapsedMilliseconds(at: .nan)
        }
        expectPacerFailure(.invalidTimestamp, "infinite frame timestamp") {
            _ = try pacer.elapsedMilliseconds(at: .infinity)
        }
        expectPacerFailure(.frameAdvanceTooLarge, "background frame gap") {
            _ = try pacer.elapsedMilliseconds(at: 10.4)
        }
        let recoveredPacedDelta = try pacer.elapsedMilliseconds(at: 10.046875)
        precondition(recoveredPacedDelta == 15, "rejected samples changed the pacing baseline")
        pacer.reset()
        let resetPacedDelta = try pacer.elapsedMilliseconds(at: 1_000)
        precondition(resetPacedDelta == 0)

        let fresh = try store.openSession(makeRuntime: makeRuntime)
        let session = try TinyArcadeGameSessionV1(restored: fresh)
        var gameplayPacer = try TinyArcadeFramePacerV1()
        try session.setButtons(.primary, forSource: 1)
        try session.setButtons(.right, forSource: 2)
        precondition(session.input.buttons == [.primary, .right])
        let launch = try session.tick(
            elapsedMilliseconds: gameplayPacer.elapsedMilliseconds(at: 20)
        )
        guard case .indexed2D = launch.renderFrame else {
            preconditionFailure("Paddle Guard must render indexed2d")
        }
        try session.setButtons([], forSource: 1)
        precondition(session.input.buttons == .right)
        _ = try session.tick(
            elapsedMilliseconds: gameplayPacer.elapsedMilliseconds(at: 20.015625)
        )
        precondition(session.gameClockMilliseconds == 15)
        try session.deactivateAndSave(to: store)
        precondition(!session.isActive)
        precondition(session.input.buttons.isEmpty)
        expectSessionFailure(.inactive, "inactive tick") {
            _ = try session.tick(elapsedMilliseconds: 0)
        }
        expectSessionFailure(.inactive, "inactive input") {
            try session.setButtons(.left, forSource: 3)
        }
        try session.close()
        expectSessionFailure(.closed, "closed session") {
            _ = try session.tick(elapsedMilliseconds: 0)
        }

        let restored = try store.openSession(makeRuntime: makeRuntime)
        precondition(restored.disposition == .restored)
        precondition(restored.gameClockMilliseconds == 15)
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
        precondition(resumed.gameClockMilliseconds == 15)
        try resumed.setButtons(.right, forSource: 2)
        _ = try resumed.tick(elapsedMilliseconds: 16)
        precondition(resumed.gameClockMilliseconds == 31)
        try resumed.deactivateAndSave(to: store)
        precondition(!resumed.isActive)
        try resumed.activate()
        gameplayPacer.reset()
        _ = try resumed.tick(
            elapsedMilliseconds: gameplayPacer.elapsedMilliseconds(at: 2_000)
        )
        precondition(resumed.gameClockMilliseconds == 31)
        try resumed.deactivateAndSave(to: store)
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
        precondition(finalRestore.gameClockMilliseconds == 31)
        try finalRestore.runtime.close()

        let unsafeDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-session-save-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: unsafeDirectory) }
        let unsafeStore = try TinyArcadeSnapshotStoreV1(directoryURL: unsafeDirectory)
        let storageFailureSession = try TinyArcadeGameSessionV1(runtime: makeRuntime())
        let unsafeSnapshot = unsafeDirectory
            .appendingPathComponent("com.partnernet.paddle-guard.snapshot-v1")
        try FileManager.default.createSymbolicLink(
            at: unsafeSnapshot,
            withDestinationURL: unsafeDirectory.appendingPathComponent("missing")
        )
        do {
            try storageFailureSession.deactivateAndSave(to: unsafeStore)
            preconditionFailure("unsafe save destination must fail")
        } catch let error as TinyArcadeSnapshotStoreError {
            precondition(error == .unsafeStoredFile)
        }
        precondition(!storageFailureSession.isFailed)
        precondition(!storageFailureSession.isActive)
        expectSessionFailure(.inactive, "storage-failed inactive session") {
            _ = try storageFailureSession.tick(elapsedMilliseconds: 0)
        }
        try storageFailureSession.activate()
        _ = try storageFailureSession.tick(elapsedMilliseconds: 0)
        try storageFailureSession.close()

        let externallyClosedRuntime = try makeRuntime()
        let runtimeFailureSession = try TinyArcadeGameSessionV1(runtime: externallyClosedRuntime)
        try externallyClosedRuntime.close()
        do {
            try runtimeFailureSession.save(to: store)
            preconditionFailure("closed runtime save must fail")
        } catch is TinyArcadeRuntimeError {
            precondition(runtimeFailureSession.isFailed)
        }
        expectSessionFailure(.failed, "failed session after runtime save error") {
            _ = try runtimeFailureSession.tick(elapsedMilliseconds: 0)
        }
        try? runtimeFailureSession.close()

        print("OK: monotonic pacing → active/inactive session → snapshot clock restore → invalid host input recovery")
    }

    private static func expectPacerFailure(
        _ expected: TinyArcadeFramePacerError,
        _ context: String,
        _ body: () throws -> Void
    ) {
        do {
            try body()
            preconditionFailure("expected \(context) failure")
        } catch let error as TinyArcadeFramePacerError {
            precondition(error == expected)
        } catch {
            preconditionFailure("unexpected \(context) error: \(error)")
        }
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
