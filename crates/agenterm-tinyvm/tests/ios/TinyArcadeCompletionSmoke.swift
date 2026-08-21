import Foundation
import TinyArcade

@main
enum TinyArcadeCompletionSmoke {
    @MainActor
    static func main() throws {
        let completion = try TinyArcadeCompletionV1(
            module: "fan:async/v1",
            maxPending: 4,
            maxReservedBytes: 1_024
        )
        let start = TinyArcadeNativeFunctionV1(
            module: "fan:async/v1",
            field: "start",
            parameterCount: 0,
            resultCount: 1
        ) { _, _ in
            [try completion.begin(maxPayloadBytes: 256)]
        }
        let profile = try TinyArcadeHostProfileV1.appBuild(
            nativeFunctions: [start],
            completionChannels: [completion]
        )
        precondition(!profile.encoded.isEmpty)
        try completion.close()
    }
}
