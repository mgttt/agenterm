import Foundation
import TinyWasiHost

@main
private struct TinyWasiHostSmoke {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("WASI host smoke requires fixture .wasm")
        }
        let wasm = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyvm-wasi-host-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = [UInt8](directory.path.utf8)
        var config = tinyvm_wasi_host_config_v1()
        precondition(tinyvm_wasi_host_v1_default_config(&config) == TINYVM_WASI_HOST_OK)
        config.max_memory_pages = 4
        config.max_steps = 100_000
        var didExit: UInt32 = 0
        var exitCode: UInt32 = 0
        let status = wasm.withUnsafeBytes { wasmBytes in
            path.withUnsafeBytes { pathBytes in
                tinyvm_wasi_host_v1_run(
                    wasmBytes.bindMemory(to: UInt8.self).baseAddress,
                    wasmBytes.count,
                    pathBytes.bindMemory(to: UInt8.self).baseAddress,
                    pathBytes.count,
                    &config,
                    &didExit,
                    &exitCode
                )
            }
        }
        precondition(status == TINYVM_WASI_HOST_OK)
        precondition(didExit == 1)
        precondition(exitCode == 7)
        let saved = try Data(contentsOf: directory.appendingPathComponent("slot.bin"))
        precondition(saved == Data("hello".utf8))
        print("OK: standard WASI _start wrote /save/slot.bin in the iOS container and exited 7")
    }
}
