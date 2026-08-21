import Foundation
import TinyArcade

@main
enum TinyArcadeCompletionSmoke {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("usage: TinyArcadeCompletionSmoke CARTRIDGE.wasm")
        }
        let cartridge = try Data(
            contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]),
            options: .mappedIfSafe
        )
        let completion = try TinyArcadeCompletionV1(
            module: "fan:async/v1",
            maxPending: 4,
            maxReservedBytes: 1_024
        )
        var startedTicket: Int32 = 0
        let start = TinyArcadeNativeFunctionV1(
            module: "fan:async/v1",
            field: "start",
            parameterCount: 0,
            resultCount: 1
        ) { _, _ in
            let ticket = try completion.begin(maxPayloadBytes: 4)
            startedTicket = ticket
            return [ticket]
        }
        let profile = try TinyArcadeHostProfileV1.appBuild(
            nativeFunctions: [start],
            completionChannels: [completion]
        )
        precondition(!profile.encoded.isEmpty)
        _ = try profile.inspectCompatibleCartridge(cartridge)

        let runtime = try TinyArcadeRuntimeV1(
            cartridge: cartridge,
            nativeFunctions: [start],
            completionChannels: [completion]
        )
        precondition(startedTicket != 0)

        let pending = try runtime.tickMedia(buttons: 0, clockMilliseconds: 0)
        try expectPixel(pending, rgba: Data([0, 0, 0, 0]))

        let payload = Data([0x11, 0x22, 0x33, 0xff])
        try completion.complete(ticket: startedTicket, status: 7, payload: payload)
        let ready = try runtime.tickMedia(buttons: 0, clockMilliseconds: 16)
        try expectPixel(ready, rgba: payload)
        expectFailure("ticket consumed by guest") {
            try completion.complete(ticket: startedTicket, status: 7, payload: payload)
        }

        try runtime.close()
        expectFailure("late result after runtime close") {
            try completion.complete(ticket: startedTicket, status: 7, payload: payload)
        }
        try completion.close()
        print("OK: Swift MainActor completion → guest poll/take → indexed frame → safe teardown")
    }

    @MainActor
    private static func expectPixel(_ frame: TinyArcadeMediaFrame, rgba: Data) throws {
        guard case let .indexed2D(indexed) = frame.renderFrame else {
            preconditionFailure("completion fixture did not emit indexed2d")
        }
        precondition(indexed.width == 1 && indexed.height == 1)
        precondition(indexed.rgba8888() == rgba)
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
