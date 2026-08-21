import Foundation
import TinyArcade

@main
private struct TinyArcadeSnapshotStoreSmoke {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("snapshot store smoke requires Paddle Guard .wasm")
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
            .appendingPathComponent("tinyarcade-snapshot-store-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = try TinyArcadeSnapshotStoreV1(
            directoryURL: directory,
            maximumSnapshotBytes: 1_024
        )
        let gameID = "com.partnernet.paddle-guard"
        let file = directory.appendingPathComponent("\(gameID).snapshot-v1")

        let fresh = try store.openSession(makeRuntime: makeRuntime)
        precondition(fresh.disposition == .fresh)
        precondition(fresh.gameClockMilliseconds == 0)
        _ = try fresh.runtime.tickMedia(buttons: 1 << 4, clockMilliseconds: 0)
        try store.save(runtime: fresh.runtime, gameClockMilliseconds: 123)
        _ = try fresh.runtime.tickMedia(buttons: 1 << 0, clockMilliseconds: 16)
        try store.save(runtime: fresh.runtime, gameClockMilliseconds: 456)
        try fresh.runtime.close()

        let restored = try store.openSession(makeRuntime: makeRuntime)
        precondition(restored.disposition == .restored)
        precondition(restored.gameClockMilliseconds == 456)
        _ = try restored.runtime.tickMedia(buttons: 0, clockMilliseconds: 472)
        try restored.runtime.close()

        var incompatible = try Data(contentsOf: file)
        let idLength = Int(incompatible[12]) | Int(incompatible[13]) << 8
        incompatible[32 + idLength] ^= 0xff
        for offset in 0..<4 { incompatible[20 + offset] = 0 }
        let repairedChecksum = checksum(incompatible)
        for offset in 0..<4 {
            incompatible[20 + offset] = UInt8(
                truncatingIfNeeded: repairedChecksum >> (offset * 8)
            )
        }
        try incompatible.write(to: file)
        let incompatibleRecovered = try store.openSession(makeRuntime: makeRuntime)
        precondition(incompatibleRecovered.disposition == .discardedInvalid)
        _ = try incompatibleRecovered.runtime.tickMedia(buttons: 0, clockMilliseconds: 0)
        try store.save(runtime: incompatibleRecovered.runtime, gameClockMilliseconds: 9)
        try incompatibleRecovered.runtime.close()

        var corrupt = try Data(contentsOf: file)
        corrupt[corrupt.index(before: corrupt.endIndex)] ^= 0xff
        try corrupt.write(to: file)
        let recovered = try store.openSession(makeRuntime: makeRuntime)
        precondition(recovered.disposition == .discardedInvalid)
        precondition(recovered.gameClockMilliseconds == 0)
        precondition(!FileManager.default.fileExists(atPath: file.path))
        _ = try recovered.runtime.tickMedia(buttons: 0, clockMilliseconds: 0)
        try recovered.runtime.close()

        try Data(count: 1_300).write(to: file)
        let oversized = try store.openSession(makeRuntime: makeRuntime)
        precondition(oversized.disposition == .discardedInvalid)
        precondition(!FileManager.default.fileExists(atPath: file.path))
        try oversized.runtime.close()

        try FileManager.default.createSymbolicLink(at: file, withDestinationURL: URL(fileURLWithPath: "/dev/null"))
        do {
            _ = try store.openSession(makeRuntime: makeRuntime)
            preconditionFailure("snapshot symlink must fail closed")
        } catch let error as TinyArcadeSnapshotStoreError {
            precondition(error == .unsafeStoredFile)
        }
        try FileManager.default.removeItem(at: file)
        try FileManager.default.createSymbolicLink(
            at: file,
            withDestinationURL: directory.appendingPathComponent("missing.snapshot-v1")
        )
        do {
            _ = try store.openSession(makeRuntime: makeRuntime)
            preconditionFailure("dangling snapshot symlink must fail closed")
        } catch let error as TinyArcadeSnapshotStoreError {
            precondition(error == .unsafeStoredFile)
        }
        try FileManager.default.removeItem(at: file)
        print("OK: atomic snapshot overwrite → restore → corrupt/oversize recovery → symlink refusal")
    }

    static func checksum(_ data: Data) -> UInt32 {
        var crc = UInt32.max
        for (offset, stored) in data.enumerated() {
            let byte: UInt8 = (20..<24).contains(offset) ? 0 : stored
            crc ^= UInt32(byte)
            for _ in 0..<8 { crc = (crc >> 1) ^ (0xedb8_8320 & (0 &- (crc & 1))) }
        }
        return ~crc
    }
}
