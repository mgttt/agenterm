import Foundation
import TinyArcade

@main
private struct TinyArcadePrivateLibrarySmoke {
    @MainActor
    static func main() throws {
        guard CommandLine.arguments.count == 3 else {
            preconditionFailure("private-library smoke requires Depth Well and Paddle Guard")
        }
        let depth = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let paddle = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-private-library-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let library = try TinyArcadePrivateLibraryV1(
            directoryURL: directory,
            maximumCartridgeBytes: 32 * 1_024
        )

        let paddleItem = try library.importCartridge(paddle) { config in
            config.max_memory_pages = 17
            config.max_steps = 500_000
            config.max_render_bytes = 20 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 128
        }
        precondition(paddleItem.gameID == "com.partnernet.paddle-guard")
        precondition(paddleItem.gameVersion == "0.1.0")
        precondition(paddleItem.fileURL.lastPathComponent == "com.partnernet.paddle-guard@0.1.0.wasm")
        let installedPaddleBytes = try Data(contentsOf: paddleItem.fileURL)
        precondition(installedPaddleBytes == paddle)

        let opened = try library.open(paddleItem) { config in
            config.max_memory_pages = 17
            config.max_steps = 500_000
            config.max_render_bytes = 20 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 128
        }
        let openedOrigin = try opened.origin()
        precondition(openedOrigin == .privateUser)
        _ = try opened.tickMedia(buttons: 1 << 4, clockMilliseconds: 0)
        try opened.close()

        expectFailure("invalid update must not replace good bytes") {
            _ = try library.importCartridge(Data([0]))
        }
        let preservedPaddleBytes = try Data(contentsOf: paddleItem.fileURL)
        precondition(preservedPaddleBytes == paddle)
        _ = try library.importCartridge(paddle)

        let depthItem = try library.importCartridge(depth) { config in
            config.max_memory_pages = 17
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        let installed = try library.installedCartridges()
        precondition(installed.map(\.gameID) == [
            "com.partnernet.depth-well",
            "com.partnernet.paddle-guard",
        ])

        var corrupt = paddle
        corrupt[0] ^= 0xff
        try corrupt.write(to: paddleItem.fileURL, options: .atomic)
        expectFailure("corrupt stored cartridge") { _ = try library.open(paddleItem) }
        _ = try library.importCartridge(paddle)

        try Data(count: 33 * 1_024).write(to: paddleItem.fileURL, options: .atomic)
        expectPrivateFailure(.invalidLimit, "oversized stored cartridge") {
            _ = try library.open(paddleItem)
        }
        _ = try library.importCartridge(paddle)

        try FileManager.default.removeItem(at: paddleItem.fileURL)
        try FileManager.default.createSymbolicLink(
            at: paddleItem.fileURL,
            withDestinationURL: depthItem.fileURL
        )
        expectPrivateFailure(.unsafeStoredFile, "symlinked private cartridge") {
            _ = try library.installedCartridges()
        }
        try FileManager.default.removeItem(at: paddleItem.fileURL)
        try FileManager.default.createSymbolicLink(
            at: paddleItem.fileURL,
            withDestinationURL: directory.appendingPathComponent("missing.wasm")
        )
        expectPrivateFailure(.unsafeStoredFile, "dangling-symlink private cartridge") {
            _ = try library.importCartridge(paddle)
        }
        try FileManager.default.removeItem(at: paddleItem.fileURL)
        _ = try library.importCartridge(paddle)

        try library.remove(depthItem)
        let remaining = try library.installedCartridges()
        precondition(remaining.map(\.gameID) == [paddleItem.gameID])
        try library.remove(paddleItem)
        let empty = try library.installedCartridges()
        precondition(empty.isEmpty)

        for index in 0..<TinyArcadePrivateLibraryV1.maximumCartridgeCount {
            let leaf = String(format: "fan%03d@1.wasm", index)
            try Data([0]).write(to: directory.appendingPathComponent(leaf))
        }
        expectPrivateFailure(.tooManyCartridges, "full private library") {
            _ = try library.importCartridge(paddle)
        }

        print("OK: private import → atomic update → enumerate/open → corruption/symlink/count refusal → remove")
    }

    @MainActor
    private static func expectFailure(_ context: String, _ body: () throws -> Void) {
        do {
            try body()
            preconditionFailure("expected failure: \(context)")
        } catch {
            precondition(!String(describing: error).isEmpty)
        }
    }

    @MainActor
    private static func expectPrivateFailure(
        _ expected: TinyArcadePrivateLibraryError,
        _ context: String,
        _ body: () throws -> Void
    ) {
        do {
            try body()
            preconditionFailure("expected failure: \(context)")
        } catch let error as TinyArcadePrivateLibraryError {
            precondition(error == expected, "unexpected \(context) error: \(error)")
        } catch {
            preconditionFailure("unexpected \(context) error: \(error)")
        }
    }
}
