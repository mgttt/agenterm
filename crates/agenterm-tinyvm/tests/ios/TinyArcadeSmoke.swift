import Foundation
import TinyArcade

@main
struct TinyArcadeSmoke {
    @MainActor
    static func main() throws {
        precondition(tinyarcade_v1_abi_version() == TINYARCADE_ABI_VERSION)
        var config = tinyarcade_config_v1()
        precondition(tinyarcade_v1_default_config(&config) == TINYARCADE_OK)
        precondition(config.struct_size == MemoryLayout<tinyarcade_config_v1>.size)
        _ = TinyArcadeRuntimeV1.self

        guard CommandLine.arguments.count == 2 else { return }
        let cartridge = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let runtime = try TinyArcadeRuntimeV1(privateCartridge: cartridge) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        let origin = try runtime.origin()
        precondition(origin == .privateUser)
        let frame = try runtime.tick(buttons: 0, clockMilliseconds: 0)
        precondition(frame.grid3D.width == 5)
        precondition(frame.grid3D.depth == 5)
        precondition(frame.grid3D.height == 10)
        precondition(frame.grid3D.cells.count == 8)
        precondition(frame.tones.isEmpty)
        let snapshot = try runtime.suspend()
        let restored = try TinyArcadeRuntimeV1(privateCartridge: cartridge) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        try restored.resume(snapshot: snapshot)
        let dropped = try restored.tick(buttons: 1 << 7, clockMilliseconds: 1)
        precondition(dropped.grid3D.score >= 10)
        precondition(dropped.tones.count == 1)

        let measured = try TinyArcadeRuntimeV1(privateCartridge: cartridge) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        var milliseconds: [Double] = []
        milliseconds.reserveCapacity(600)
        for index in 0..<600 {
            let started = ProcessInfo.processInfo.systemUptime
            _ = try measured.tick(buttons: 0, clockMilliseconds: UInt32(index * 16))
            milliseconds.append((ProcessInfo.processInfo.systemUptime - started) * 1_000)
        }
        milliseconds.sort()
        let average = milliseconds.reduce(0, +) / Double(milliseconds.count)
        let p95 = milliseconds[Int(Double(milliseconds.count - 1) * 0.95)]
        let maximum = milliseconds.last ?? 0
        precondition(p95 < 8, "Depth Well simulator p95 exceeded 8 ms")
        print(
            "OK: Depth Well in iOS Simulator; "
                + String(format: "600 frames avg=%.3fms p95=%.3fms max=%.3fms", average, p95, maximum)
        )
    }
}
