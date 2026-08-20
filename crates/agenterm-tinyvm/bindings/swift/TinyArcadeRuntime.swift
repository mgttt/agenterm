import Foundation
import AVFoundation
import CoreGraphics
import UIKit
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

public struct TinyArcadeIndexed2DFrame: Sendable {
    public let width: UInt16
    public let height: UInt16
    public let paletteRGBA: [UInt32]
    public let pixels: Data

    fileprivate init(data: Data) throws {
        guard data.count >= 16,
              data.count <= 64 * 1_024,
              data.prefix(4) == Data("TAI2".utf8),
              TinyArcadeGrid3DFrame.u16(data, 4) == 1,
              TinyArcadeGrid3DFrame.u16(data, 6) == 16 else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid indexed2d frame header")
        }
        width = TinyArcadeGrid3DFrame.u16(data, 8)
        height = TinyArcadeGrid3DFrame.u16(data, 10)
        let paletteCount = Int(TinyArcadeGrid3DFrame.u16(data, 12))
        let flags = TinyArcadeGrid3DFrame.u16(data, 14)
        let pixelCount = Int(width) * Int(height)
        let pixelOffset = 16 + paletteCount * 4
        guard width > 0, height > 0,
              width <= 512, height <= 512,
              pixelCount <= Int(UInt16.max),
              (1...256).contains(paletteCount),
              flags == 0,
              data.count == pixelOffset + pixelCount else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid indexed2d frame size")
        }
        var decodedPalette: [UInt32] = []
        decodedPalette.reserveCapacity(paletteCount)
        for index in 0..<paletteCount {
            decodedPalette.append(TinyArcadeGrid3DFrame.u32(data, 16 + index * 4))
        }
        paletteRGBA = decodedPalette
        pixels = data.subdata(in: pixelOffset..<data.count)
        guard !pixels.contains(where: { Int($0) >= paletteCount }) else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid indexed2d pixel")
        }
    }

    /// Copies the indexed plane into canonical row-major RGBA8 bytes.
    /// The decoded frame bounds this allocation to less than 256 KiB.
    public func rgba8888() -> Data {
        var bytes = [UInt8]()
        bytes.reserveCapacity(pixels.count * 4)
        for index in pixels {
            let color = paletteRGBA[Int(index)]
            bytes.append(UInt8(truncatingIfNeeded: color))
            bytes.append(UInt8(truncatingIfNeeded: color >> 8))
            bytes.append(UInt8(truncatingIfNeeded: color >> 16))
            bytes.append(UInt8(truncatingIfNeeded: color >> 24))
        }
        return Data(bytes)
    }

    /// Builds an sRGB, non-premultiplied RGBA image suitable for Core Graphics
    /// or direct assignment to a Core Animation layer.
    public func makeCGImage() throws -> CGImage {
        let rgba = rgba8888()
        guard let provider = CGDataProvider(data: rgba as CFData),
              let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw TinyArcadePresentationError.imageAllocation
        }
        let bitmapInfo = CGBitmapInfo.byteOrder32Big.union(
            CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
        )
        guard let image = CGImage(
            width: Int(width),
            height: Int(height),
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: Int(width) * 4,
            space: colorSpace,
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        ) else {
            throw TinyArcadePresentationError.imageAllocation
        }
        return image
    }
}

public enum TinyArcadePresentationError: Error, Equatable {
    case imageAllocation
}

/// Minimal native presentation surface for indexed cartridges. The host owns
/// its layout; this view preserves aspect ratio and nearest-neighbour pixels.
@MainActor
public final class TinyArcadeIndexed2DView: UIView {
    public override init(frame: CGRect) {
        super.init(frame: frame)
        configure()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        configure()
    }

    public func display(_ frame: TinyArcadeIndexed2DFrame) throws {
        layer.contents = try frame.makeCGImage()
    }

    public func clear() {
        layer.contents = nil
    }

    private func configure() {
        isOpaque = false
        clipsToBounds = true
        layer.contentsGravity = .resizeAspect
        layer.magnificationFilter = .nearest
        layer.minificationFilter = .nearest
    }
}

public struct TinyArcadeToneEvent: Sendable, Equatable {
    public static let maximumBatchEventCount = 16
    public static let maximumBatchDurationMilliseconds: UInt32 = 4_000

    /// Stable host feedback intent: 1 impact, 2 success, 3 failure.
    public let kind: UInt8
    public let frequencyHz: UInt16
    public let durationMilliseconds: UInt16
    public let amplitudeMilli: UInt16

    fileprivate static func decodeBatch(_ audio: Data) throws -> [TinyArcadeToneEvent] {
        if audio.isEmpty { return [] }
        guard audio.count >= 8,
              audio.prefix(4) == Data("TAT1".utf8),
              TinyArcadeGrid3DFrame.u16(audio, 4) == 1 else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid tone batch header")
        }
        let count = Int(TinyArcadeGrid3DFrame.u16(audio, 6))
        guard count <= maximumBatchEventCount, audio.count == 8 + count * 8 else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid tone batch size")
        }
        var decoded: [TinyArcadeToneEvent] = []
        decoded.reserveCapacity(count)
        var totalDurationMilliseconds: UInt32 = 0
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
            totalDurationMilliseconds += UInt32(event.durationMilliseconds)
            decoded.append(event)
        }
        guard totalDurationMilliseconds <= maximumBatchDurationMilliseconds else {
            throw TinyArcadeGrid3DFrame.decodeError("invalid tone batch duration")
        }
        return decoded
    }
}

/// Bounded native rendering for `tinyarcade:tones/v1` events.
/// Events remain semantic hints: hosts may choose another timbre while retaining
/// their order, pitch, duration and relative amplitude.
public enum TinyArcadeToneSynthesizer {
    public static let sampleRate: UInt32 = 22_050

    public static func waveData(for events: [TinyArcadeToneEvent]) -> Data {
        let gapSamples = Int(sampleRate) * 4 / 1_000
        let eventSamples = events.reduce(into: 0) { total, event in
            total += Int(sampleRate) * Int(event.durationMilliseconds) / 1_000
        }
        let sampleCount = eventSamples + max(0, events.count - 1) * gapSamples
        var pcm = Data(capacity: sampleCount * MemoryLayout<Int16>.size)

        for (eventIndex, event) in events.enumerated() {
            let count = Int(sampleRate) * Int(event.durationMilliseconds) / 1_000
            let attack = max(1, min(count / 2, Int(sampleRate) * 3 / 1_000))
            let release = max(1, min(count / 2, Int(sampleRate) * 8 / 1_000))
            let amplitude = Double(event.amplitudeMilli) / 1_000.0 * 0.28
            let radiansPerSample = 2.0 * Double.pi * Double(event.frequencyHz)
                / Double(sampleRate)

            for sampleIndex in 0..<count {
                let attackEnvelope = min(1.0, Double(sampleIndex + 1) / Double(attack))
                let releaseEnvelope = min(1.0, Double(count - sampleIndex) / Double(release))
                let envelope = min(attackEnvelope, releaseEnvelope)
                let sine = sin(Double(sampleIndex) * radiansPerSample)
                let square = sine >= 0 ? 1.0 : -1.0
                let shape: Double
                switch event.kind {
                case 1: shape = sine * 0.55 + square * 0.45
                case 2: shape = sine
                default: shape = sine * 0.8 + square * 0.2
                }
                let value = Int16(clamping: Int(shape * envelope * amplitude * 32_767.0))
                appendLittleEndian(value, to: &pcm)
            }
            if eventIndex + 1 < events.count {
                pcm.append(Data(count: gapSamples * MemoryLayout<Int16>.size))
            }
        }

        var wave = Data(capacity: 44 + pcm.count)
        wave.append(contentsOf: "RIFF".utf8)
        appendLittleEndian(UInt32(36 + pcm.count), to: &wave)
        wave.append(contentsOf: "WAVEfmt ".utf8)
        appendLittleEndian(UInt32(16), to: &wave)
        appendLittleEndian(UInt16(1), to: &wave)
        appendLittleEndian(UInt16(1), to: &wave)
        appendLittleEndian(sampleRate, to: &wave)
        appendLittleEndian(sampleRate * 2, to: &wave)
        appendLittleEndian(UInt16(2), to: &wave)
        appendLittleEndian(UInt16(16), to: &wave)
        wave.append(contentsOf: "data".utf8)
        appendLittleEndian(UInt32(pcm.count), to: &wave)
        wave.append(pcm)
        return wave
    }

    private static func appendLittleEndian<T: FixedWidthInteger>(_ value: T, to data: inout Data) {
        withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) }
    }
}

public enum TinyArcadeTonePlayerError: Error, Equatable {
    case playbackUnavailable
}

/// Main-actor owner for short native tone batches. A new batch replaces the
/// current one; interruption never resumes stale gameplay feedback.
@MainActor
public final class TinyArcadeTonePlayer {
    private let managesAudioSession: Bool
    private let audioSession = AVAudioSession.sharedInstance()
    private var player: AVAudioPlayer?

    public private(set) var isAudioSessionActive = false
    public var isPlaying: Bool { player?.isPlaying ?? false }

    public init(managesAudioSession: Bool = true) {
        self.managesAudioSession = managesAudioSession
    }

    public func play(_ events: [TinyArcadeToneEvent]) throws {
        guard !events.isEmpty else { return }
        stop()
        var activatedForAttempt = false
        if managesAudioSession && !isAudioSessionActive {
            try audioSession.setCategory(.ambient, mode: .default, options: [.mixWithOthers])
            try audioSession.setActive(true)
            isAudioSessionActive = true
            activatedForAttempt = true
        }
        do {
            let next = try AVAudioPlayer(data: TinyArcadeToneSynthesizer.waveData(for: events))
            next.prepareToPlay()
            guard next.play() else { throw TinyArcadeTonePlayerError.playbackUnavailable }
            player = next
        } catch {
            if activatedForAttempt {
                try? audioSession.setActive(false, options: [.notifyOthersOnDeactivation])
                isAudioSessionActive = false
            }
            throw error
        }
    }

    public func stop() {
        player?.stop()
        player = nil
    }

    /// Forward `AVAudioSession.interruptionNotification` began events here.
    public func interruptionBegan() {
        stop()
        if managesAudioSession { isAudioSessionActive = false }
    }

    public func deactivate() throws {
        stop()
        if managesAudioSession && isAudioSessionActive {
            try audioSession.setActive(false, options: [.notifyOthersOnDeactivation])
            isAudioSessionActive = false
        }
    }
}

public enum TinyArcadeRenderFrame: Sendable {
    case grid3D(TinyArcadeGrid3DFrame)
    case indexed2D(TinyArcadeIndexed2DFrame)

    fileprivate init(data: Data) throws {
        if data.prefix(4) == Data("TAG3".utf8) {
            self = .grid3D(try TinyArcadeGrid3DFrame(data: data))
        } else if data.prefix(4) == Data("TAI2".utf8) {
            self = .indexed2D(try TinyArcadeIndexed2DFrame(data: data))
        } else {
            throw TinyArcadeGrid3DFrame.decodeError("unknown render stream")
        }
    }
}

public struct TinyArcadeMediaFrame: Sendable {
    public let render: Data
    public let audio: Data
    public let renderFrame: TinyArcadeRenderFrame
    public let tones: [TinyArcadeToneEvent]

    fileprivate init(render: Data, audio: Data) throws {
        self.render = render
        self.audio = audio
        renderFrame = try TinyArcadeRenderFrame(data: render)
        tones = try TinyArcadeToneEvent.decodeBatch(audio)
    }
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
        tones = try TinyArcadeToneEvent.decodeBatch(audio)
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

/// One exact, versioned native capability exposed to a bundled or reviewed cartridge.
/// The handler runs synchronously on the runtime owner thread. It must not retain `memory`.
public struct TinyArcadeNativeFunctionV1 {
    public let module: String
    public let field: String
    public let parameterCount: UInt32
    public let resultCount: UInt32
    public let maxCallsPerLifecycle: UInt32
    public let handler: ([Int32], UnsafeMutableRawBufferPointer) throws -> [Int32]

    public init(
        module: String,
        field: String,
        parameterCount: UInt32,
        resultCount: UInt32,
        maxCallsPerLifecycle: UInt32 = 1,
        handler: @escaping ([Int32], UnsafeMutableRawBufferPointer) throws -> [Int32]
    ) {
        self.module = module
        self.field = field
        self.parameterCount = parameterCount
        self.resultCount = resultCount
        self.maxCallsPerLifecycle = maxCallsPerLifecycle
        self.handler = handler
    }
}

private final class TinyArcadeNativeCallbackBox {
    let modulePointer: UnsafeMutablePointer<UInt8>
    let moduleCount: Int
    let fieldPointer: UnsafeMutablePointer<UInt8>
    let fieldCount: Int
    let parameterCount: UInt32
    let resultCount: UInt32
    let maxCallsPerLifecycle: UInt32
    let handler: ([Int32], UnsafeMutableRawBufferPointer) throws -> [Int32]

    init(_ function: TinyArcadeNativeFunctionV1) throws {
        let module = Array(function.module.utf8)
        let field = Array(function.field.utf8)
        guard !module.isEmpty, !field.isEmpty,
              function.parameterCount <= 16, function.resultCount <= 16,
              (1...64).contains(function.maxCallsPerLifecycle) else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "native imports require non-empty UTF-8 names, at most 16 parameters/results and 1...64 calls per lifecycle"
            )
        }
        let ownedModule = UnsafeMutablePointer<UInt8>.allocate(capacity: module.count)
        module.withUnsafeBufferPointer { bytes in
            ownedModule.initialize(from: bytes.baseAddress!, count: bytes.count)
        }
        let ownedField = UnsafeMutablePointer<UInt8>.allocate(capacity: field.count)
        field.withUnsafeBufferPointer { bytes in
            ownedField.initialize(from: bytes.baseAddress!, count: bytes.count)
        }
        modulePointer = ownedModule
        fieldPointer = ownedField
        moduleCount = module.count
        fieldCount = field.count
        parameterCount = function.parameterCount
        resultCount = function.resultCount
        maxCallsPerLifecycle = function.maxCallsPerLifecycle
        handler = function.handler
    }

    deinit {
        modulePointer.deinitialize(count: moduleCount)
        modulePointer.deallocate()
        fieldPointer.deinitialize(count: fieldCount)
        fieldPointer.deallocate()
    }

    func descriptor() -> tinyarcade_native_function_v1 {
        tinyarcade_native_function_v1(
            struct_size: UInt32(MemoryLayout<tinyarcade_native_function_v1>.size),
            module: UnsafePointer(modulePointer),
            module_len: moduleCount,
            field: UnsafePointer(fieldPointer),
            field_len: fieldCount,
            n_params: parameterCount,
            n_results: resultCount,
            max_calls_per_lifecycle: maxCallsPerLifecycle,
            callback: tinyArcadeNativeCallback,
            context: Unmanaged.passUnretained(self).toOpaque()
        )
    }
}

private func tinyArcadeNativeCallback(
    context: UnsafeMutableRawPointer?,
    params: UnsafePointer<Int32>?,
    parameterCount: Int,
    results: UnsafeMutablePointer<Int32>?,
    resultCount: Int,
    memory: UnsafeMutablePointer<UInt8>?,
    memoryCount: Int
) -> Int32 {
    guard let context,
          parameterCount == 0 || params != nil,
          resultCount == 0 || results != nil,
          memoryCount == 0 || memory != nil else { return -1 }
    let box = Unmanaged<TinyArcadeNativeCallbackBox>.fromOpaque(context).takeUnretainedValue()
    guard parameterCount == Int(box.parameterCount), resultCount == Int(box.resultCount) else {
        return -1
    }
    do {
        let parameters = params.map { Array(UnsafeBufferPointer(start: $0, count: parameterCount)) } ?? []
        let guestMemory = UnsafeMutableRawBufferPointer(start: memory, count: memoryCount)
        let returned = try box.handler(parameters, guestMemory)
        guard returned.count == resultCount else { return -1 }
        if let results {
            for (index, value) in returned.enumerated() {
                results[index] = value
            }
        }
        return 0
    } catch {
        return -1
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
    private var nativeCallbackBoxes: [TinyArcadeNativeCallbackBox] = []

    public init(
        cartridge: Data,
        nativeFunctions: [TinyArcadeNativeFunctionV1] = [],
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        if nativeFunctions.isEmpty {
            handle = try Self.open(
                cartridge: cartridge,
                configure: configure,
                function: tinyarcade_v1_open
            )
        } else {
            let opened = try Self.openWithNativeFunctions(
                cartridge: cartridge,
                nativeFunctions: nativeFunctions,
                configure: configure
            )
            handle = opened.handle
            nativeCallbackBoxes = opened.boxes
        }
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
        nativeFunctions: [TinyArcadeNativeFunctionV1] = [],
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
        let status = try gameID.withUnsafeBytes { gameIDBytes in
            try gameVersion.withUnsafeBytes { versionBytes in
                try entry.wasmSHA256.withUnsafeBytes { hashBytes in
                    try keyID.withUnsafeBytes { keyIDBytes in
                        try entry.signature.withUnsafeBytes { signatureBytes in
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
                            return try cartridge.withUnsafeBytes { cartridgeBytes in
                                if nativeFunctions.isEmpty {
                                    return tinyarcade_v1_open_reviewed(
                                        cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                                        cartridgeBytes.count,
                                        &cEntry,
                                        trust,
                                        &config,
                                        &opened
                                    )
                                }
                                return try Self.withNativeFunctionTable(nativeFunctions) { table, count, boxes in
                                    let result = tinyarcade_v1_open_reviewed_with_native_modules(
                                        cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                                        cartridgeBytes.count,
                                        &cEntry,
                                        trust,
                                        table,
                                        count,
                                        &config,
                                        &opened
                                    )
                                    if result == TINYARCADE_OK {
                                        nativeCallbackBoxes = boxes
                                    }
                                    return result
                                }
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
        nativeCallbackBoxes.removeAll()
    }

    public func tick(buttons: UInt32, clockMilliseconds: UInt32) throws -> TinyArcadeFrame {
        let output = try tickOutput(buttons: buttons, clockMilliseconds: clockMilliseconds)
        return try TinyArcadeFrame(render: output.render, audio: output.audio)
    }

    /// Runs one lifecycle tick and decodes any standard TinyArcade render stream.
    /// Existing 3D-only consumers may continue to use `tick`.
    public func tickMedia(buttons: UInt32, clockMilliseconds: UInt32) throws -> TinyArcadeMediaFrame {
        let output = try tickOutput(buttons: buttons, clockMilliseconds: clockMilliseconds)
        return try TinyArcadeMediaFrame(render: output.render, audio: output.audio)
    }

    private func tickOutput(
        buttons: UInt32,
        clockMilliseconds: UInt32
    ) throws -> (render: Data, audio: Data) {
        let handle = try liveHandle()
        try Self.check(tinyarcade_v1_tick(handle, buttons, clockMilliseconds))
        let render = try copy(handle, tinyarcade_v1_copy_render)
        let audio = try copy(handle, tinyarcade_v1_copy_audio)
        return (render, audio)
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

    private static func openWithNativeFunctions(
        cartridge: Data,
        nativeFunctions: [TinyArcadeNativeFunctionV1],
        configure: (inout tinyarcade_config_v1) -> Void
    ) throws -> (handle: OpaquePointer, boxes: [TinyArcadeNativeCallbackBox]) {
        var config = tinyarcade_config_v1()
        try check(tinyarcade_v1_default_config(&config))
        configure(&config)
        var opened: OpaquePointer?
        var retainedBoxes: [TinyArcadeNativeCallbackBox] = []
        let status = try cartridge.withUnsafeBytes { bytes in
            try withNativeFunctionTable(nativeFunctions) { table, count, boxes in
                retainedBoxes = boxes
                return tinyarcade_v1_open_with_native_modules(
                    bytes.bindMemory(to: UInt8.self).baseAddress,
                    bytes.count,
                    table,
                    count,
                    &config,
                    &opened
                )
            }
        }
        try check(status)
        return (try requireHandle(opened), retainedBoxes)
    }

    private static func withNativeFunctionTable<T>(
        _ functions: [TinyArcadeNativeFunctionV1],
        _ body: (
            UnsafePointer<tinyarcade_native_function_v1>?,
            Int,
            [TinyArcadeNativeCallbackBox]
        ) throws -> T
    ) throws -> T {
        guard functions.count <= 64 else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "a runtime accepts at most 64 native functions"
            )
        }
        let boxes = try functions.map(TinyArcadeNativeCallbackBox.init)
        let descriptors = boxes.map { $0.descriptor() }
        return try descriptors.withUnsafeBufferPointer { table in
            try body(table.baseAddress, table.count, boxes)
        }
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
