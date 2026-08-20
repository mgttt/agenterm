import Foundation
import TinyArcade

@main
struct TinyArcadeSmoke {
    static func appendLEB(_ value: Int, to output: inout [UInt8]) {
        var remaining = value
        repeat {
            var byte = UInt8(remaining & 0x7f)
            remaining >>= 7
            if remaining != 0 { byte |= 0x80 }
            output.append(byte)
        } while remaining != 0
    }

    static func appendName(_ value: String, to output: inout [UInt8]) {
        let bytes = Array(value.utf8)
        appendLEB(bytes.count, to: &output)
        output.append(contentsOf: bytes)
    }

    static func appendSection(_ id: UInt8, _ payload: [UInt8], to module: inout [UInt8]) {
        module.append(id)
        appendLEB(payload.count, to: &module)
        module.append(contentsOf: payload)
    }

    static func functionBody(_ code: [UInt8]) -> [UInt8] {
        [0] + code
    }

    static func nativeCartridge() -> Data {
        var module: [UInt8] = [0, 97, 115, 109, 1, 0, 0, 0]
        let capability = "fan:physics/v1"
        var manifest: [UInt8] = []
        appendName("tinyarcade.manifest.v1", to: &manifest)
        manifest += Array("TAM1".utf8) + [1, 0, 0, 0, 1, 0, 0, 0]
        for value in ["c.native", "1.0.0"] {
            let bytes = Array(value.utf8)
            manifest += [UInt8(bytes.count), 0] + bytes
        }
        manifest += [1, 0, UInt8(capability.utf8.count), 0] + Array(capability.utf8)
        appendSection(0, manifest, to: &module)
        appendSection(1, [2, 0x60, 0, 1, 0x7f, 0x60, 2, 0x7f, 0x7f, 1, 0x7f], to: &module)
        var imports: [UInt8] = [2]
        for (namespace, field) in [
            (capability, "step_world"),
            ("tinyarcade:core/v1", "submit_render"),
        ] {
            appendName(namespace, to: &imports)
            appendName(field, to: &imports)
            imports += [0, 1]
        }
        appendSection(2, imports, to: &module)
        appendSection(3, [5, 0, 0, 0, 0, 0], to: &module)
        appendSection(5, [1, 0, 1], to: &module)
        var exports: [UInt8] = [5]
        for (field, index) in [
            ("game_abi_version", 2),
            ("game_init", 3),
            ("game_tick", 4),
            ("game_suspend", 5),
            ("game_resume", 6),
        ] {
            appendName(field, to: &exports)
            exports.append(0)
            appendLEB(index, to: &exports)
        }
        appendSection(7, exports, to: &module)
        let functions = [
            functionBody([0x41, 1, 0x0b]),
            functionBody([0x41, 0, 0x0b]),
            functionBody([
                0x41, 0, 0x41, 0x28, 0x41, 2, 0x10, 0, 0x36, 2, 0,
                0x41, 0, 0x41, 8, 0x10, 1, 0x1a, 0x41, 0, 0x0b,
            ]),
            functionBody([0x41, 0, 0x0b]),
            functionBody([0x41, 0, 0x0b]),
        ]
        var code: [UInt8] = [5]
        for function in functions {
            appendLEB(function.count, to: &code)
            code += function
        }
        appendSection(10, code, to: &module)
        return Data(module)
    }

    @MainActor
    static func main() throws {
        precondition(tinyarcade_v1_abi_version() == TINYARCADE_ABI_VERSION)
        var config = tinyarcade_config_v1()
        precondition(tinyarcade_v1_default_config(&config) == TINYARCADE_OK)
        precondition(config.struct_size == MemoryLayout<tinyarcade_config_v1>.size)
        _ = TinyArcadeRuntimeV1.self

        var nativeCalls = 0
        let nativeRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1
                ) { parameters, memory in
                    precondition(parameters == [40, 2])
                    precondition(memory.count >= 8)
                    memory[4] = 9
                    nativeCalls += 1
                    return [42]
                },
            ]
        )
        do {
            _ = try nativeRuntime.tick(buttons: 0, clockMilliseconds: 0)
            preconditionFailure("native smoke frame should not decode as grid3d")
        } catch {
            precondition(nativeCalls == 1)
        }
        try nativeRuntime.close()

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
