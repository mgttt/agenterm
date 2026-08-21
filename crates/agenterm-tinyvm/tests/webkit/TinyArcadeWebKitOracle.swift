import CryptoKit
import Darwin
import Foundation
import JavaScriptCore

private enum OracleError: Error, CustomStringConvertible {
    case invalid(String)

    var description: String {
        switch self {
        case let .invalid(message): message
        }
    }
}

private struct ReplayStep {
    let buttons: UInt32
    let clockMs: UInt32
    let renderLength: UInt32
    let audioLength: UInt32
    let renderSHA256: Data
    let audioSHA256: Data
}

private struct Replay {
    let cartridgeSHA256: Data
    let rng: UInt32
    let guestState: Data
    let steps: [ReplayStep]
}

private struct Cursor {
    let data: Data
    var offset = 0

    mutating func take(_ count: Int) throws -> Data {
        guard count >= 0, offset <= data.count, count <= data.count - offset else {
            throw OracleError.invalid("truncated replay")
        }
        defer { offset += count }
        return data.subdata(in: offset ..< offset + count)
    }

    mutating func u16() throws -> UInt16 {
        let bytes = [UInt8](try take(2))
        return UInt16(bytes[0]) | UInt16(bytes[1]) << 8
    }

    mutating func u32() throws -> UInt32 {
        let bytes = [UInt8](try take(4))
        return UInt32(bytes[0]) | UInt32(bytes[1]) << 8
            | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
    }
}

private func parseSnapshot(_ data: Data) throws -> (UInt32, Data) {
    var cursor = Cursor(data: data)
    guard try cursor.take(4) == Data("TGS1".utf8) else {
        throw OracleError.invalid("invalid snapshot magic")
    }
    _ = try cursor.u32()
    _ = try cursor.u32()
    let gameIDLength = Int(try cursor.u16())
    _ = try cursor.take(gameIDLength)
    let rng = try cursor.u32()
    let guestLength = Int(try cursor.u32())
    let guest = try cursor.take(guestLength)
    guard cursor.offset == data.count else {
        throw OracleError.invalid("trailing snapshot bytes")
    }
    return (rng, guest)
}

private func parseReplay(_ data: Data) throws -> Replay {
    guard (64 ... 8 * 1_024 * 1_024).contains(data.count) else {
        throw OracleError.invalid("replay size")
    }
    var cursor = Cursor(data: data)
    guard try cursor.take(4) == Data("TAR1".utf8), try cursor.u16() == 1,
          try cursor.u16() == 64 else {
        throw OracleError.invalid("invalid replay header")
    }
    let cartridgeSHA256 = try cursor.take(32)
    _ = try cursor.u32()
    _ = try cursor.u32()
    let gameIDLength = Int(try cursor.u16())
    let gameVersionLength = Int(try cursor.u16())
    let snapshotLength = Int(try cursor.u32())
    let stepCount = Int(try cursor.u32())
    guard try cursor.u32() == 0, (1 ... 128).contains(gameIDLength),
          (1 ... 64).contains(gameVersionLength),
          (1 ... 1_024 * 1_024).contains(snapshotLength), stepCount <= 65_536 else {
        throw OracleError.invalid("invalid replay bounds")
    }
    _ = try cursor.take(gameIDLength)
    _ = try cursor.take(gameVersionLength)
    let snapshot = try cursor.take(snapshotLength)
    var steps: [ReplayStep] = []
    steps.reserveCapacity(stepCount)
    for _ in 0 ..< stepCount {
        steps.append(
            ReplayStep(
                buttons: try cursor.u32(),
                clockMs: try cursor.u32(),
                renderLength: try cursor.u32(),
                audioLength: try cursor.u32(),
                renderSHA256: try cursor.take(32),
                audioSHA256: try cursor.take(32)
            )
        )
    }
    guard cursor.offset == data.count else {
        throw OracleError.invalid("invalid replay length")
    }
    let (rng, guestState) = try parseSnapshot(snapshot)
    return Replay(
        cartridgeSHA256: cartridgeSHA256,
        rng: rng,
        guestState: guestState,
        steps: steps
    )
}

private func numbers(_ data: Data) -> [NSNumber] {
    data.map { NSNumber(value: $0) }
}

private func frameData(_ value: JSValue?, field: String) throws -> Data {
    guard let values = value?.forProperty(field)?.toArray() as? [NSNumber] else {
        throw OracleError.invalid("WebKit oracle returned invalid \(field)")
    }
    var bytes = Data(capacity: values.count)
    for value in values {
        let byte = value.intValue
        guard (0 ... 255).contains(byte) else {
            throw OracleError.invalid("WebKit oracle returned non-byte \(field)")
        }
        bytes.append(UInt8(byte))
    }
    return bytes
}

@main
private struct TinyArcadeWebKitOracle {
    static func main() {
        do {
            try run()
        } catch {
            FileHandle.standardError.write(Data("webkit-oracle: \(error)\n".utf8))
            exit(1)
        }
    }

    private static func run() throws {
        guard CommandLine.arguments.count == 4 else {
            throw OracleError.invalid("usage: ORACLE adapter.js cartridge.wasm trace.tareplay")
        }
        let adapter = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
        let wasm = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
        let replay = try parseReplay(
            Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3]))
        )
        guard Data(SHA256.hash(data: wasm)) == replay.cartridgeSHA256 else {
            throw OracleError.invalid("replay/cartridge SHA-256 mismatch")
        }

        guard let context = JSContext() else {
            throw OracleError.invalid("cannot create JavaScriptCore context")
        }
        var exception: String?
        context.exceptionHandler = { _, value in exception = value?.toString() }
        context.evaluateScript(adapter)
        if let exception { throw OracleError.invalid("adapter load: \(exception)") }
        guard let oracle = context.objectForKeyedSubscript("runTinyArcadeWebKitOracle") else {
            throw OracleError.invalid("adapter did not export oracle")
        }
        let configuration: [String: Any] = [
            "rng": NSNumber(value: replay.rng),
            "guestState": numbers(replay.guestState),
            "steps": replay.steps.map {
                [
                    "buttons": NSNumber(value: $0.buttons),
                    "clockMs": NSNumber(value: $0.clockMs),
                ]
            },
        ]
        let result = oracle.call(withArguments: [numbers(wasm), configuration])
        if let exception { throw OracleError.invalid("WebKit execution: \(exception)") }
        guard let frames = result?.forProperty("frames"),
              frames.toArray()?.count == replay.steps.count else {
            throw OracleError.invalid("WebKit oracle returned wrong frame count")
        }

        for (index, expected) in replay.steps.enumerated() {
            let frame = frames.atIndex(index)
            let render = try frameData(frame, field: "render")
            let audio = try frameData(frame, field: "audio")
            guard render.count == Int(expected.renderLength),
                  audio.count == Int(expected.audioLength),
                  Data(SHA256.hash(data: render)) == expected.renderSHA256,
                  Data(SHA256.hash(data: audio)) == expected.audioSHA256 else {
                throw OracleError.invalid("engine mismatch at replay step \(index)")
            }
        }
        print(
            "OK: JavaScriptCore WebAssembly == tinyvm for \(replay.steps.count) exact frames"
        )
    }
}
