import Foundation
import UIKit
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

    static func appendSignedLEB(_ value: Int, to output: inout [UInt8]) {
        precondition(value >= 0)
        var remaining = value
        while true {
            var byte = UInt8(remaining & 0x7f)
            remaining >>= 7
            let done = remaining == 0 && byte & 0x40 == 0
            if !done { byte |= 0x80 }
            output.append(byte)
            if done { return }
        }
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

    static func nativeCartridge(renderLength: Int = 26) -> Data {
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
        var imports: [UInt8] = [3]
        for (namespace, field, typeIndex) in [
            (capability, "step_world", UInt8(1)),
            ("tinyarcade:core/v1", "indexed2d_version", UInt8(0)),
            ("tinyarcade:core/v1", "submit_render", UInt8(1)),
        ] {
            appendName(namespace, to: &imports)
            appendName(field, to: &imports)
            imports += [0, typeIndex]
        }
        appendSection(2, imports, to: &module)
        appendSection(3, [5, 0, 0, 0, 0, 0], to: &module)
        appendSection(5, [1, 0, 1], to: &module)
        var exports: [UInt8] = [5]
        for (field, index) in [
            ("game_abi_version", 3),
            ("game_init", 4),
            ("game_tick", 5),
            ("game_suspend", 6),
            ("game_resume", 7),
        ] {
            appendName(field, to: &exports)
            exports.append(0)
            appendLEB(index, to: &exports)
        }
        appendSection(7, exports, to: &module)
        var tick: [UInt8] = [
            0x10, 1, 0x1a,
            0x41, 0x28, 0x41, 2, 0x10, 0, 0x1a,
            0x41, 0x28, 0x41, 2, 0x10, 0, 0x1a,
            0x41, 0,
            0x41,
        ]
        appendSignedLEB(renderLength, to: &tick)
        tick += [0x10, 2, 0x1a, 0x41, 0, 0x0b]
        let functions = [
            functionBody([0x41, 1, 0x0b]),
            functionBody([0x41, 0, 0x0b]),
            functionBody(tick),
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

    static func indexedFrame(lastPixel: UInt8) -> [UInt8] {
        [
            84, 65, 73, 50, 1, 0, 16, 0,
            2, 0, 1, 0, 2, 0, 0, 0,
            255, 0, 0, 255, 0, 255, 0, 128,
            0, lastPixel,
        ]
    }

    static func classicIndexedFrame() -> [UInt8] {
        let width = 320
        let height = 200
        var bytes: [UInt8] = [
            84, 65, 73, 50, 1, 0, 16, 0,
            UInt8(width & 0xff), UInt8(width >> 8), UInt8(height), 0,
            0, 1, 0, 0,
        ]
        bytes.reserveCapacity(16 + 256 * 4 + width * height)
        for color in 0..<256 {
            bytes += [UInt8(color), UInt8(255 - color), UInt8(color ^ 0x55), 255]
        }
        for pixel in 0..<(width * height) {
            bytes.append(UInt8(pixel & 0xff))
        }
        return bytes
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
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { parameters, memory in
                    precondition(parameters == [40, 2])
                    let indexedFrame = Self.indexedFrame(lastPixel: 1)
                    precondition(memory.count >= indexedFrame.count)
                    for (index, value) in indexedFrame.enumerated() { memory[index] = value }
                    nativeCalls += 1
                    return [42]
                },
            ]
        )
        let media = try nativeRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
        precondition(nativeCalls == 2)
        guard case let .indexed2D(indexedFrame) = media.renderFrame else {
            preconditionFailure("native smoke should decode indexed2d")
        }
        precondition(indexedFrame.width == 2 && indexedFrame.height == 1)
        precondition(indexedFrame.paletteRGBA == [0xff00_00ff, 0x8000_ff00])
        precondition(indexedFrame.pixels == Data([0, 1]))
        let expectedRGBA = Data([255, 0, 0, 255, 0, 255, 0, 128])
        precondition(indexedFrame.rgba8888() == expectedRGBA)
        let image = try indexedFrame.makeCGImage()
        precondition(image.width == 2 && image.height == 1)
        precondition(image.bitsPerPixel == 32 && image.bytesPerRow == 8)
        precondition(image.shouldInterpolate == false)
        precondition(image.alphaInfo == .last)
        precondition(image.bitmapInfo.contains(.byteOrder32Big))
        precondition(image.colorSpace?.name == CGColorSpace.sRGB)
        guard let providerData = image.dataProvider?.data else {
            preconditionFailure("indexed image must retain its pixel provider")
        }
        precondition(providerData as Data == expectedRGBA)
        let view = TinyArcadeIndexed2DView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        try view.display(indexedFrame)
        precondition(view.layer.contents != nil)
        precondition(view.layer.contentsGravity == .resizeAspect)
        precondition(view.layer.magnificationFilter == .nearest)
        precondition(view.layer.minificationFilter == .nearest)
        view.clear()
        precondition(view.layer.contents == nil)
        try nativeRuntime.close()

        let malformedRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { _, memory in
                    let malformed = Self.indexedFrame(lastPixel: 2)
                    for (index, value) in malformed.enumerated() { memory[index] = value }
                    return [42]
                },
            ]
        )
        do {
            _ = try malformedRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
            preconditionFailure("out-of-palette indexed pixel must fail")
        } catch let error as TinyArcadeRuntimeError {
            precondition(error.status == Int32(TINYARCADE_DECODE_ERROR.rawValue))
        }
        try malformedRuntime.close()

        let classicBytes = Self.classicIndexedFrame()
        precondition(classicBytes.count == 65_040)
        let classicRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(renderLength: classicBytes.count),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { _, memory in
                    for (index, value) in classicBytes.enumerated() { memory[index] = value }
                    return [42]
                },
            ]
        )
        let classicMedia = try classicRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
        guard case let .indexed2D(classicFrame) = classicMedia.renderFrame else {
            preconditionFailure("classic smoke should decode indexed2d")
        }
        precondition(classicFrame.width == 320 && classicFrame.height == 200)
        precondition(classicFrame.paletteRGBA.count == 256)
        precondition(classicFrame.pixels.count == 64_000)
        let classicView = TinyArcadeIndexed2DView(
            frame: CGRect(x: 0, y: 0, width: 390, height: 844)
        )
        let renderIterations = 120
        let renderStart = ProcessInfo.processInfo.systemUptime
        for _ in 0..<renderIterations { try classicView.display(classicFrame) }
        let renderAverageMilliseconds = (
            ProcessInfo.processInfo.systemUptime - renderStart
        ) * 1_000 / Double(renderIterations)
        precondition(renderAverageMilliseconds < 16.0)
        print(
            String(
                format: "OK: indexed2d 320x200 native presentation avg=%.3fms",
                renderAverageMilliseconds
            )
        )
        try classicRuntime.close()

        guard CommandLine.arguments.count >= 2 else { return }
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
        try runtime.close()
        try restored.close()
        try measured.close()

        guard CommandLine.arguments.count >= 3 else { return }
        let paddleCartridge = try Data(
            contentsOf: URL(fileURLWithPath: CommandLine.arguments[2])
        )
        let makePaddleRuntime: () throws -> TinyArcadeRuntimeV1 = {
            try TinyArcadeRuntimeV1(privateCartridge: paddleCartridge) { config in
                config.max_memory_pages = 17
                config.max_steps = 500_000
                config.max_render_bytes = 20 * 1_024
                config.max_audio_bytes = 64
                config.max_state_bytes = 128
            }
        }
        var paddleRuntime = try makePaddleRuntime()
        var paddleFrame = try paddleRuntime.tickMedia(
            buttons: 1 << 4,
            clockMilliseconds: 0
        )
        guard case let .indexed2D(initialPaddle) = paddleFrame.renderFrame else {
            preconditionFailure("Paddle Guard must emit indexed2d")
        }
        precondition(initialPaddle.width == 160 && initialPaddle.height == 120)
        precondition(initialPaddle.paletteRGBA.count == 8)
        let paddleView = TinyArcadeIndexed2DView(
            frame: CGRect(x: 0, y: 0, width: 390, height: 844)
        )
        try paddleView.display(initialPaddle)
        var paddleMilliseconds: [Double] = []
        paddleMilliseconds.reserveCapacity(600)
        var sawPaddleTone = !paddleFrame.tones.isEmpty
        for index in 1...600 {
            if index == 300 {
                let saved = try paddleRuntime.suspend()
                let resumed = try makePaddleRuntime()
                try resumed.resume(snapshot: saved)
                try paddleRuntime.close()
                paddleRuntime = resumed
            }
            let buttons: UInt32 = (index / 90).isMultiple(of: 2) ? 1 << 0 : 1 << 1
            let started = ProcessInfo.processInfo.systemUptime
            paddleFrame = try paddleRuntime.tickMedia(
                buttons: buttons,
                clockMilliseconds: UInt32(index * 16)
            )
            guard case let .indexed2D(decoded) = paddleFrame.renderFrame else {
                preconditionFailure("Paddle Guard changed render protocol")
            }
            try paddleView.display(decoded)
            paddleMilliseconds.append((ProcessInfo.processInfo.systemUptime - started) * 1_000)
            sawPaddleTone = sawPaddleTone || !paddleFrame.tones.isEmpty
        }
        paddleMilliseconds.sort()
        let paddleAverage = paddleMilliseconds.reduce(0, +) / Double(paddleMilliseconds.count)
        let paddleP95 = paddleMilliseconds[Int(Double(paddleMilliseconds.count - 1) * 0.95)]
        let paddleMaximum = paddleMilliseconds.last ?? 0
        precondition(sawPaddleTone, "Paddle Guard must emit gameplay feedback")
        precondition(paddleP95 < 8, "Paddle Guard simulator p95 exceeded 8 ms")
        print(
            "OK: Paddle Guard in iOS Simulator; "
                + String(
                    format: "600 frames avg=%.3fms p95=%.3fms max=%.3fms",
                    paddleAverage,
                    paddleP95,
                    paddleMaximum
                )
        )
        try paddleRuntime.close()
    }
}
