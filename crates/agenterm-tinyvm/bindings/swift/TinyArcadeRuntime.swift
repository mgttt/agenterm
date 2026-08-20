import Foundation
import TinyArcade

public struct TinyArcadeRuntimeError: Error, Sendable {
    public let status: Int32
    public let message: String
}

public struct TinyArcadeGridCell: Sendable, Equatable {
    public let x: UInt8
    public let y: UInt8
    public let z: UInt8
    public let kind: UInt8
    public let rgba: UInt32
}

public struct TinyArcadeGrid3DFrame: Sendable {
    public let width: UInt16
    public let depth: UInt16
    public let height: UInt16
    public let score: UInt32
    public let clearedDecks: UInt32
    public let level: UInt32
    public let isGameOver: Bool
    public let cells: [TinyArcadeGridCell]

    fileprivate init(data: Data) throws {
        guard data.count >= 32,
              data.prefix(4) == Data("TAG3".utf8),
              Self.u16(data, 4) == 1,
              Self.u16(data, 6) == 32 else {
            throw Self.decodeError("invalid grid3d frame header")
        }
        width = Self.u16(data, 8)
        depth = Self.u16(data, 10)
        height = Self.u16(data, 12)
        let count = Int(Self.u16(data, 14))
        score = Self.u32(data, 16)
        clearedDecks = Self.u32(data, 20)
        level = Self.u32(data, 24)
        let flags = Self.u32(data, 28)
        guard width > 0, depth > 0, height > 0,
              flags & ~UInt32(1) == 0,
              data.count == 32 + count * 8 else {
            throw Self.decodeError("invalid grid3d frame size or flags")
        }
        isGameOver = flags & 1 != 0
        var decoded: [TinyArcadeGridCell] = []
        decoded.reserveCapacity(count)
        for index in 0..<count {
            let offset = 32 + index * 8
            let cell = TinyArcadeGridCell(
                x: data[offset],
                y: data[offset + 1],
                z: data[offset + 2],
                kind: data[offset + 3],
                rgba: Self.u32(data, offset + 4)
            )
            guard UInt16(cell.x) < width,
                  UInt16(cell.y) < depth,
                  UInt16(cell.z) < height,
                  (1...3).contains(cell.kind) else {
                throw Self.decodeError("invalid grid3d cell")
            }
            decoded.append(cell)
        }
        cells = decoded
    }

    fileprivate static func u16(_ data: Data, _ offset: Int) -> UInt16 {
        UInt16(data[offset]) | UInt16(data[offset + 1]) << 8
    }

    fileprivate static func u32(_ data: Data, _ offset: Int) -> UInt32 {
        UInt32(data[offset])
            | UInt32(data[offset + 1]) << 8
            | UInt32(data[offset + 2]) << 16
            | UInt32(data[offset + 3]) << 24
    }

    fileprivate static func decodeError(_ message: String) -> TinyArcadeRuntimeError {
        TinyArcadeRuntimeError(
            status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
            message: message
        )
    }
}

public struct TinyArcadeToneEvent: Sendable, Equatable {
    public let kind: UInt8
    public let frequencyHz: UInt16
    public let durationMilliseconds: UInt16
    public let amplitudeMilli: UInt16
}

public struct TinyArcadeFrame: Sendable {
    public let render: Data
    public let audio: Data
    public let grid3D: TinyArcadeGrid3DFrame
    public let tones: [TinyArcadeToneEvent]

    fileprivate init(render: Data, audio: Data) throws {
        self.render = render
        self.audio = audio
        grid3D = try TinyArcadeGrid3DFrame(data: render)
        if audio.isEmpty {
            tones = []
            return
        }
        guard audio.count >= 8,
              audio.prefix(4) == Data("TAT1".utf8),
              TinyArcadeGrid3DFrame.u16(audio, 4) == 1 else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid tone batch header")
        }
        let count = Int(TinyArcadeGrid3DFrame.u16(audio, 6))
        guard audio.count == 8 + count * 8 else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid tone batch size")
        }
        var decoded: [TinyArcadeToneEvent] = []
        decoded.reserveCapacity(count)
        for index in 0..<count {
            let offset = 8 + index * 8
            let event = TinyArcadeToneEvent(
                kind: audio[offset],
                frequencyHz: TinyArcadeGrid3DFrame.u16(audio, offset + 2),
                durationMilliseconds: TinyArcadeGrid3DFrame.u16(audio, offset + 4),
                amplitudeMilli: TinyArcadeGrid3DFrame.u16(audio, offset + 6)
            )
            guard audio[offset + 1] == 0,
                  (1...3).contains(event.kind),
                  (40...20_000).contains(event.frequencyHz),
                  (1...2_000).contains(event.durationMilliseconds),
                  event.amplitudeMilli <= 1_000 else {
                throw TinyArcadeGrid3DFrame.decodeError("invalid tone event")
            }
            decoded.append(event)
        }
        tones = decoded
    }
}

public enum TinyArcadeCartridgeOrigin: UInt32, Sendable {
    case bundled = 0
    case officialReviewed = 1
    case privateUser = 2
}

public struct TinyArcadeReviewedCatalogEntry: Sendable {
    public let gameID: String
    public let gameVersion: String
    public let abiVersion: UInt32
    public let stateVersion: UInt32
    public let wasmLength: UInt64
    public let wasmSHA256: Data
    public let signingKeyID: String
    public let signature: Data

    public init(
        gameID: String,
        gameVersion: String,
        abiVersion: UInt32,
        stateVersion: UInt32,
        wasmLength: UInt64,
        wasmSHA256: Data,
        signingKeyID: String,
        signature: Data
    ) {
        self.gameID = gameID
        self.gameVersion = gameVersion
        self.abiVersion = abiVersion
        self.stateVersion = stateVersion
        self.wasmLength = wasmLength
        self.wasmSHA256 = wasmSHA256
        self.signingKeyID = signingKeyID
        self.signature = signature
    }
}

/// Main-actor owner for official catalog keys and live revocations.
@MainActor
public final class TinyArcadeTrustStoreV1 {
    fileprivate var handle: OpaquePointer?

    public init() throws {
        var opened: OpaquePointer?
        try TinyArcadeRuntimeV1.check(tinyarcade_v1_trust_store_create(&opened))
        guard let opened else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "runtime returned a null trust store"
            )
        }
        handle = opened
    }

    isolated deinit {
        if let handle {
            _ = tinyarcade_v1_trust_store_close(handle)
        }
    }

    public func addKey(id: String, ed25519PublicKey: Data) throws {
        let handle = try liveHandle()
        let keyID = Data(id.utf8)
        let status = keyID.withUnsafeBytes { idBytes in
            ed25519PublicKey.withUnsafeBytes { keyBytes in
                tinyarcade_v1_trust_store_add_key(
                    handle,
                    idBytes.bindMemory(to: UInt8.self).baseAddress,
                    idBytes.count,
                    keyBytes.bindMemory(to: UInt8.self).baseAddress,
                    keyBytes.count
                )
            }
        }
        try TinyArcadeRuntimeV1.check(status)
    }

    public func revokeKey(id: String) throws {
        let handle = try liveHandle()
        let keyID = Data(id.utf8)
        let status = keyID.withUnsafeBytes { bytes in
            tinyarcade_v1_trust_store_revoke_key(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count
            )
        }
        try TinyArcadeRuntimeV1.check(status)
    }

    public func revokeContent(sha256: Data) throws {
        let handle = try liveHandle()
        let status = sha256.withUnsafeBytes { bytes in
            tinyarcade_v1_trust_store_revoke_content(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count
            )
        }
        try TinyArcadeRuntimeV1.check(status)
    }

    public func close() throws {
        guard let handle else { return }
        try TinyArcadeRuntimeV1.check(tinyarcade_v1_trust_store_close(handle))
        self.handle = nil
    }

    fileprivate func liveHandle() throws -> OpaquePointer {
        guard let handle else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "trust store is closed"
            )
        }
        return handle
    }
}

/// Main-actor owner for the single-threaded C runtime handle.
@MainActor
public final class TinyArcadeRuntimeV1 {
    private var handle: OpaquePointer?

    public init(
        cartridge: Data,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        handle = try Self.open(
            cartridge: cartridge,
            configure: configure,
            function: tinyarcade_v1_open
        )
    }

    public init(
        privateCartridge cartridge: Data,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        handle = try Self.open(
            cartridge: cartridge,
            configure: configure,
            function: tinyarcade_v1_open_private
        )
    }

    public init(
        reviewedCartridge cartridge: Data,
        entry: TinyArcadeReviewedCatalogEntry,
        trustStore: TinyArcadeTrustStoreV1,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        guard entry.wasmSHA256.count == 32, entry.signature.count == 64 else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "invalid reviewed catalog hash or signature length"
            )
        }
        var config = tinyarcade_config_v1()
        try Self.check(tinyarcade_v1_default_config(&config))
        configure(&config)
        var opened: OpaquePointer?
        let gameID = Data(entry.gameID.utf8)
        let gameVersion = Data(entry.gameVersion.utf8)
        let keyID = Data(entry.signingKeyID.utf8)
        let trust = try trustStore.liveHandle()
        let status = gameID.withUnsafeBytes { gameIDBytes in
            gameVersion.withUnsafeBytes { versionBytes in
                entry.wasmSHA256.withUnsafeBytes { hashBytes in
                    keyID.withUnsafeBytes { keyIDBytes in
                        entry.signature.withUnsafeBytes { signatureBytes in
                            var cEntry = tinyarcade_catalog_entry_v1(
                                struct_size: UInt32(MemoryLayout<tinyarcade_catalog_entry_v1>.size),
                                game_id: gameIDBytes.bindMemory(to: UInt8.self).baseAddress,
                                game_id_len: gameIDBytes.count,
                                game_version: versionBytes.bindMemory(to: UInt8.self).baseAddress,
                                game_version_len: versionBytes.count,
                                abi_version: entry.abiVersion,
                                state_version: entry.stateVersion,
                                wasm_length: entry.wasmLength,
                                wasm_sha256: hashBytes.bindMemory(to: UInt8.self).baseAddress,
                                wasm_sha256_len: hashBytes.count,
                                signing_key_id: keyIDBytes.bindMemory(to: UInt8.self).baseAddress,
                                signing_key_id_len: keyIDBytes.count,
                                signature: signatureBytes.bindMemory(to: UInt8.self).baseAddress,
                                signature_len: signatureBytes.count
                            )
                            return cartridge.withUnsafeBytes { cartridgeBytes in
                                tinyarcade_v1_open_reviewed(
                                    cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                                    cartridgeBytes.count,
                                    &cEntry,
                                    trust,
                                    &config,
                                    &opened
                                )
                            }
                        }
                    }
                }
            }
        }
        try Self.check(status)
        handle = try Self.requireHandle(opened)
    }

    isolated deinit {
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
        let render = try copy(handle, tinyarcade_v1_copy_render)
        let audio = try copy(handle, tinyarcade_v1_copy_audio)
        return try TinyArcadeFrame(render: render, audio: audio)
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

    public func origin() throws -> TinyArcadeCartridgeOrigin {
        var raw: UInt32 = 0
        try Self.check(tinyarcade_v1_origin(try liveHandle(), &raw))
        guard let value = TinyArcadeCartridgeOrigin(rawValue: raw) else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                message: "runtime returned an unknown cartridge origin"
            )
        }
        return value
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

    private typealias OpenFunction = @convention(c) (
        UnsafePointer<UInt8>?, Int, UnsafePointer<tinyarcade_config_v1>?,
        UnsafeMutablePointer<OpaquePointer?>?
    ) -> tinyarcade_status_v1

    private static func open(
        cartridge: Data,
        configure: (inout tinyarcade_config_v1) -> Void,
        function: OpenFunction
    ) throws -> OpaquePointer {
        var config = tinyarcade_config_v1()
        try check(tinyarcade_v1_default_config(&config))
        configure(&config)
        var opened: OpaquePointer?
        let status = cartridge.withUnsafeBytes { bytes in
            function(
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                &config,
                &opened
            )
        }
        try check(status)
        return try requireHandle(opened)
    }

    private static func requireHandle(_ handle: OpaquePointer?) throws -> OpaquePointer {
        guard let handle else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "runtime returned a null handle"
            )
        }
        return handle
    }

    fileprivate static func check(_ status: tinyarcade_status_v1) throws {
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
