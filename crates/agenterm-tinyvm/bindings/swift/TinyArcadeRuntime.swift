import Foundation
import TinyArcade

public struct TinyArcadeRuntimeError: Error, Sendable {
    public let status: Int32
    public let message: String
}

public struct TinyArcadeFrame: Sendable {
    public let render: Data
    public let audio: Data
}

/// Main-actor owner for the single-threaded C runtime handle.
@MainActor
public final class TinyArcadeRuntimeV1 {
    private var handle: OpaquePointer?

    public init(
        cartridge: Data,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        var config = tinyarcade_config_v1()
        try Self.check(tinyarcade_v1_default_config(&config))
        configure(&config)
        var opened: OpaquePointer?
        let status = cartridge.withUnsafeBytes { bytes in
            tinyarcade_v1_open(
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                &config,
                &opened
            )
        }
        try Self.check(status)
        guard let opened else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "runtime returned a null handle"
            )
        }
        handle = opened
    }

    deinit {
        if let handle {
            _ = tinyarcade_v1_close(handle)
        }
    }

    public func close() throws {
        guard let handle else { return }
        try Self.check(tinyarcade_v1_close(handle))
        self.handle = nil
    }

    public func tick(buttons: UInt32, clockMilliseconds: UInt32) throws -> TinyArcadeFrame {
        let handle = try liveHandle()
        try Self.check(tinyarcade_v1_tick(handle, buttons, clockMilliseconds))
        return TinyArcadeFrame(
            render: try copy(handle, tinyarcade_v1_copy_render),
            audio: try copy(handle, tinyarcade_v1_copy_audio)
        )
    }

    public func suspend() throws -> Data {
        let handle = try liveHandle()
        try Self.check(tinyarcade_v1_suspend(handle))
        return try copy(handle, tinyarcade_v1_copy_snapshot)
    }

    public func resume(snapshot: Data) throws {
        let handle = try liveHandle()
        let status = snapshot.withUnsafeBytes { bytes in
            tinyarcade_v1_resume(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count
            )
        }
        try Self.check(status)
    }

    public func gameID() throws -> String {
        try string(handle: liveHandle(), copyFunction: tinyarcade_v1_copy_game_id)
    }

    public func gameVersion() throws -> String {
        try string(handle: liveHandle(), copyFunction: tinyarcade_v1_copy_game_version)
    }

    private func liveHandle() throws -> OpaquePointer {
        guard let handle else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "runtime is closed"
            )
        }
        return handle
    }

    private typealias CopyFunction = @convention(c) (
        OpaquePointer?, UnsafeMutablePointer<UInt8>?, Int,
        UnsafeMutablePointer<Int>?
    ) -> tinyarcade_status_v1

    private func copy(_ handle: OpaquePointer, _ function: CopyFunction) throws -> Data {
        var count = 0
        let query = function(handle, nil, 0, &count)
        if count == 0 {
            try Self.check(query)
            return Data()
        }
        guard query == TINYARCADE_BUFFER_TOO_SMALL else {
            try Self.check(query)
            return Data()
        }
        var data = Data(count: count)
        let status = data.withUnsafeMutableBytes { bytes in
            function(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                &count
            )
        }
        try Self.check(status)
        return data
    }

    private func string(handle: OpaquePointer, copyFunction: CopyFunction) throws -> String {
        let data = try copy(handle, copyFunction)
        guard let value = String(data: data, encoding: .utf8) else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                message: "runtime metadata is not UTF-8"
            )
        }
        return value
    }

    private static func check(_ status: tinyarcade_status_v1) throws {
        guard status == TINYARCADE_OK else {
            throw TinyArcadeRuntimeError(
                status: Int32(status.rawValue),
                message: lastError()
            )
        }
    }

    private static func lastError() -> String {
        var count = 0
        let query = tinyarcade_v1_last_error(nil, 0, &count)
        guard query == TINYARCADE_BUFFER_TOO_SMALL, count > 0 else { return "tinyarcade error" }
        var bytes = [UInt8](repeating: 0, count: count)
        let status = tinyarcade_v1_last_error(&bytes, bytes.count, &count)
        guard status == TINYARCADE_OK else { return "tinyarcade error" }
        return String(decoding: bytes, as: UTF8.self)
    }
}
