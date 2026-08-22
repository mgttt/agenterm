import Foundation
import TinyArcade

@main
struct TinyArcadeSimdSmoke {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else { return }
        let cartridge = try Data(
            contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])
        )
        let runtime = try TinyArcadeRuntimeV1(
            privateCartridge: cartridge,
            distributionPolicy: .sdkTestExternalCartridges
        ) { config in
            config.max_memory_pages = 1
            config.max_steps = 10_000
            config.max_render_bytes = 64
            config.max_audio_bytes = 0
            config.max_state_bytes = 16
        }
        let frame = try runtime.tickMedia(buttons: 0, clockMilliseconds: 0)
        guard case let .indexed2D(image) = frame.renderFrame else {
            preconditionFailure("SIMD cartridge changed render protocol")
        }
        precondition(image.width == 1 && image.height == 1)
        precondition(image.paletteRGBA == [0xff00ff00])
        precondition(image.pixels == Data([0]))
        let snapshot = try runtime.suspend()
        precondition(!snapshot.isEmpty)
        try runtime.close()
        print("OK: optional SIMD audio and lane-bridge cartridge executed through Swift/C ABI on iOS Simulator")
    }
}
