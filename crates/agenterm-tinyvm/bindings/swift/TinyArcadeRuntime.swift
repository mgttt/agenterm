import Foundation
import AVFoundation
import CoreGraphics
import UIKit
import TinyArcade

public struct TinyArcadeRuntimeError: Error, Sendable {
    public let status: Int32
    public let message: String
}

public enum TinyArcadeDistributionPolicyError: Error, Sendable, Equatable {
    case externalCartridgesDisabled
    case invalidAppleApprovalReference
}

/// App-release policy for code outside the submitted app bundle. The default
/// is the App Store baseline: bundled cartridges only. Enabling external WASM
/// requires an explicit, bounded Apple approval reference for release audit.
public struct TinyArcadeDistributionPolicyV1: Sendable, Equatable {
    public static let appStoreBundledOnly = Self(
        externalApprovalReference: nil
    )

    public let externalApprovalReference: String?

    private init(externalApprovalReference: String?) {
        self.externalApprovalReference = externalApprovalReference
    }

    public static func appleApprovedExternalCartridges(
        approvalReference: String
    ) throws -> Self {
        guard (8...256).contains(approvalReference.utf8.count),
              approvalReference.utf8.allSatisfy({
                  (48...57).contains($0) || (65...90).contains($0)
                      || (97...122).contains($0) || [45, 46, 47, 58, 95].contains($0)
              }) else {
            throw TinyArcadeDistributionPolicyError.invalidAppleApprovalReference
        }
        return Self(externalApprovalReference: approvalReference)
    }

    fileprivate func requireExternalCartridges() throws {
        guard externalApprovalReference != nil else {
            throw TinyArcadeDistributionPolicyError.externalCartridgesDisabled
        }
    }

    /// Not public API: SDK black boxes exercise external paths without creating
    /// a product-facing development switch that could leak into App Store code.
    static let sdkTestExternalCartridges = Self(
        externalApprovalReference: "sdk-test-only"
    )
}

public enum TinyArcadeImportClassV1: UInt8, Sendable, Equatable {
    case core = 0
    case native = 1
}

public struct TinyArcadeFunctionImportV1: Sendable, Equatable {
    public let module: String
    public let field: String
    public let parameterCount: UInt8
    public let resultCount: UInt8
    public let importClass: TinyArcadeImportClassV1
}

/// A statically validated compatibility description. Inspection never
/// instantiates the module, runs its start function or calls guest code.
public struct TinyArcadeCartridgeDescriptorV1: Sendable, Equatable {
    public let gameID: String
    public let gameVersion: String
    public let abiVersion: UInt32
    public let stateVersion: UInt32
    public let wasmLength: UInt32
    public let nativeCapabilities: [String]
    public let functionImports: [TinyArcadeFunctionImportV1]

    public var isCoreOnly: Bool { nativeCapabilities.isEmpty }

    public static func inspect(_ cartridge: Data) throws -> Self {
        var required = 0
        let query = cartridge.withUnsafeBytes { bytes in
            tinyarcade_v1_copy_cartridge_descriptor(
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                nil,
                0,
                &required
            )
        }
        guard query == TINYARCADE_BUFFER_TOO_SMALL,
              (32...(64 * 1_024)).contains(required) else {
            try check(query)
            throw decodeError("invalid cartridge descriptor length")
        }
        var encoded = Data(count: required)
        let status = cartridge.withUnsafeBytes { cartridgeBytes in
            encoded.withUnsafeMutableBytes { outputBytes in
                tinyarcade_v1_copy_cartridge_descriptor(
                    cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                    cartridgeBytes.count,
                    outputBytes.bindMemory(to: UInt8.self).baseAddress,
                    outputBytes.count,
                    &required
                )
            }
        }
        try check(status)
        guard required == encoded.count else {
            throw decodeError("cartridge descriptor length changed")
        }
        return try decode(encoded, cartridgeLength: cartridge.count)
    }

    private static func decode(_ data: Data, cartridgeLength: Int) throws -> Self {
        guard data.count >= 32,
              data.prefix(4) == Data("TAD1".utf8),
              u16(data, 4) == 1,
              u16(data, 6) == 32,
              u32(data, 28) == 0 else {
            throw decodeError("invalid cartridge descriptor header")
        }
        let gameIDLength = Int(u16(data, 16))
        let gameVersionLength = Int(u16(data, 18))
        let capabilityCount = Int(u16(data, 20))
        let importCount = Int(u16(data, 22))
        guard (3...128).contains(gameIDLength),
              (1...64).contains(gameVersionLength),
              capabilityCount <= 64,
              importCount <= 72,
              u32(data, 24) == UInt32(exactly: cartridgeLength) else {
            throw decodeError("invalid cartridge descriptor bounds")
        }
        var cursor = 32
        let gameID = try string(data, cursor: &cursor, length: gameIDLength)
        let gameVersion = try string(data, cursor: &cursor, length: gameVersionLength)
        var capabilities: [String] = []
        capabilities.reserveCapacity(capabilityCount)
        for _ in 0..<capabilityCount {
            capabilities.append(try lengthPrefixedString(data, cursor: &cursor, maximum: 128))
        }
        var imports: [TinyArcadeFunctionImportV1] = []
        imports.reserveCapacity(importCount)
        for _ in 0..<importCount {
            guard cursor + 8 <= data.count else {
                throw decodeError("truncated cartridge import descriptor")
            }
            let moduleLength = Int(u16(data, cursor))
            let fieldLength = Int(u16(data, cursor + 2))
            let parameters = data[cursor + 4]
            let results = data[cursor + 5]
            let classValue = data[cursor + 6]
            let reserved = data[cursor + 7]
            cursor += 8
            guard moduleLength > 0, moduleLength <= 128,
                  fieldLength > 0, fieldLength <= 64,
                  parameters <= 16, results <= 16,
                  reserved == 0,
                  let importClass = TinyArcadeImportClassV1(rawValue: classValue) else {
                throw decodeError("invalid cartridge import descriptor")
            }
            let module = try string(data, cursor: &cursor, length: moduleLength)
            let field = try string(data, cursor: &cursor, length: fieldLength)
            guard (module == "tinyarcade:core/v1") == (importClass == .core) else {
                throw decodeError("invalid cartridge import class")
            }
            imports.append(
                TinyArcadeFunctionImportV1(
                    module: module,
                    field: field,
                    parameterCount: parameters,
                    resultCount: results,
                    importClass: importClass
                )
            )
        }
        guard cursor == data.count else {
            throw decodeError("trailing cartridge descriptor bytes")
        }
        return Self(
            gameID: gameID,
            gameVersion: gameVersion,
            abiVersion: u32(data, 8),
            stateVersion: u32(data, 12),
            wasmLength: u32(data, 24),
            nativeCapabilities: capabilities,
            functionImports: imports
        )
    }

    private static func lengthPrefixedString(
        _ data: Data,
        cursor: inout Int,
        maximum: Int
    ) throws -> String {
        guard cursor + 2 <= data.count else {
            throw decodeError("truncated cartridge descriptor string")
        }
        let length = Int(u16(data, cursor))
        cursor += 2
        guard length > 0, length <= maximum else {
            throw decodeError("invalid cartridge descriptor string length")
        }
        return try string(data, cursor: &cursor, length: length)
    }

    private static func string(
        _ data: Data,
        cursor: inout Int,
        length: Int
    ) throws -> String {
        guard length >= 0, cursor <= data.count, length <= data.count - cursor,
              let value = String(
                  data: data.subdata(in: cursor..<(cursor + length)),
                  encoding: .utf8
              ) else {
            throw decodeError("invalid cartridge descriptor UTF-8")
        }
        cursor += length
        return value
    }

    private static func u16(_ data: Data, _ offset: Int) -> UInt16 {
        UInt16(data[offset]) | UInt16(data[offset + 1]) << 8
    }

    private static func u32(_ data: Data, _ offset: Int) -> UInt32 {
        UInt32(data[offset]) | UInt32(data[offset + 1]) << 8
            | UInt32(data[offset + 2]) << 16 | UInt32(data[offset + 3]) << 24
    }

    private static func decodeError(_ message: String) -> TinyArcadeRuntimeError {
        TinyArcadeRuntimeError(
            status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
            message: message
        )
    }

    private static func check(_ status: tinyarcade_status_v1) throws {
        guard status == TINYARCADE_OK else {
            var count = 0
            let query = tinyarcade_v1_last_error(nil, 0, &count)
            var message = "tinyarcade descriptor error"
            if query == TINYARCADE_BUFFER_TOO_SMALL, count > 0 {
                var bytes = [UInt8](repeating: 0, count: count)
                if tinyarcade_v1_last_error(&bytes, bytes.count, &count) == TINYARCADE_OK {
                    message = String(decoding: bytes, as: UTF8.self)
                }
            }
            throw TinyArcadeRuntimeError(status: Int32(status.rawValue), message: message)
        }
    }
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

public enum TinyArcadeGameLifecycleV1: UInt32, Sendable {
    case initialize = 1
    case tick = 2
    case suspend = 3
    case resume = 4
}

/// Deterministic guest/ABI resource use for one completed lifecycle attempt.
/// Device wall time and process memory are intentionally measured separately.
public struct TinyArcadeExecutionStatsV1: Sendable, Equatable {
    public let lifecycle: TinyArcadeGameLifecycleV1
    public let wasmSteps: UInt64
    public let memoryPages: UInt32
    public let tableElements: UInt32
    public let nativeCalls: UInt32
    public let renderBytes: UInt32
    public let audioBytes: UInt32
    public let stateBytes: UInt32
}

/// ABI v1.9 deterministic stats, including VM-owned guest call-stack peaks.
public struct TinyArcadeExecutionStatsV2: Sendable, Equatable {
    public let lifecycle: TinyArcadeGameLifecycleV1
    public let wasmSteps: UInt64
    public let peakCallDepth: UInt32
    public let peakActivationSlots: UInt32
    public let memoryPages: UInt32
    public let tableElements: UInt32
    public let nativeCalls: UInt32
    public let renderBytes: UInt32
    public let audioBytes: UInt32
    public let stateBytes: UInt32
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

    fileprivate func withCEntry<T>(
        _ body: (UnsafePointer<tinyarcade_catalog_entry_v1>) throws -> T
    ) rethrows -> T {
        let gameID = Data(self.gameID.utf8)
        let gameVersion = Data(self.gameVersion.utf8)
        let keyID = Data(self.signingKeyID.utf8)
        return try gameID.withUnsafeBytes { gameIDBytes in
            try gameVersion.withUnsafeBytes { versionBytes in
                try wasmSHA256.withUnsafeBytes { hashBytes in
                    try keyID.withUnsafeBytes { keyIDBytes in
                        try signature.withUnsafeBytes { signatureBytes in
                            var entry = tinyarcade_catalog_entry_v1(
                                struct_size: UInt32(MemoryLayout<tinyarcade_catalog_entry_v1>.size),
                                game_id: gameIDBytes.bindMemory(to: UInt8.self).baseAddress,
                                game_id_len: gameIDBytes.count,
                                game_version: versionBytes.bindMemory(to: UInt8.self).baseAddress,
                                game_version_len: versionBytes.count,
                                abi_version: abiVersion,
                                state_version: stateVersion,
                                wasm_length: wasmLength,
                                wasm_sha256: hashBytes.bindMemory(to: UInt8.self).baseAddress,
                                wasm_sha256_len: hashBytes.count,
                                signing_key_id: keyIDBytes.bindMemory(to: UInt8.self).baseAddress,
                                signing_key_id_len: keyIDBytes.count,
                                signature: signatureBytes.bindMemory(to: UInt8.self).baseAddress,
                                signature_len: signatureBytes.count
                            )
                            return try withUnsafePointer(to: &entry, body)
                        }
                    }
                }
            }
        }
    }
}

public struct TinyArcadeCatalogLocalizationV1: Sendable, Equatable {
    public let title: String
    public let summary: String
}

public struct TinyArcadeCatalogHostProfileV1: Sendable, Equatable {
    public let fileURL: URL
    public let length: UInt64
    public let sha256: Data

    public init(fileURL: URL, length: UInt64, sha256: Data) {
        self.fileURL = fileURL
        self.length = length
        self.sha256 = sha256
    }
}

public struct TinyArcadeCatalogGameV1: Sendable {
    public let entry: TinyArcadeReviewedCatalogEntry
    public let title: String
    public let summary: String
    public let localizations: [String: TinyArcadeCatalogLocalizationV1]
    public let cartridgeURL: URL

    public func localized(for languageTag: String) -> TinyArcadeCatalogLocalizationV1 {
        var candidate = languageTag
        while !candidate.isEmpty {
            if let match = localizations.first(where: {
                $0.key.caseInsensitiveCompare(candidate) == .orderedSame
            })?.value {
                return match
            }
            guard let separator = candidate.lastIndex(of: "-") else { break }
            candidate.removeSubrange(separator...)
        }
        return TinyArcadeCatalogLocalizationV1(title: title, summary: summary)
    }

    public func deepLinkURL(scheme: String = "tinyarcade") -> URL? {
        var components = URLComponents()
        components.scheme = scheme
        components.host = "game"
        components.path = "/\(entry.gameID)"
        return components.url
    }
}

public enum TinyArcadeCatalogDecodeError: Error, Equatable {
    case invalidDocument
    case unsupportedSchema
    case invalidEntry(Int)
}

/// Bounded discovery metadata for an official catalog. These JSON bytes never
/// authorize execution: each selected cartridge still enters the signed-entry
/// trust store and verified cache before a reviewed runtime can open it.
public struct TinyArcadeCatalogV1: Sendable {
    public static let maximumDocumentBytes = 1 * 1_024 * 1_024
    public static let maximumGameCount = 256

    public let catalogID: String
    public let hostProfile: TinyArcadeCatalogHostProfileV1?
    public let games: [TinyArcadeCatalogGameV1]

    public static func decode(
        _ data: Data,
        cartridgeBaseURL: URL,
        maximumCartridgeBytes: UInt64 = 8 * 1_024 * 1_024
    ) throws -> TinyArcadeCatalogV1 {
        guard !data.isEmpty, data.count <= maximumDocumentBytes,
              maximumCartridgeBytes > 0,
              Self.validBaseURL(cartridgeBaseURL) else {
            throw TinyArcadeCatalogDecodeError.invalidDocument
        }
        let wire: WireCatalog
        do {
            wire = try JSONDecoder().decode(WireCatalog.self, from: data)
        } catch {
            throw TinyArcadeCatalogDecodeError.invalidDocument
        }
        guard wire.schemaVersion == 1 else {
            throw TinyArcadeCatalogDecodeError.unsupportedSchema
        }
        guard Self.canonicalIdentifier(wire.catalogID),
              !wire.games.isEmpty,
              wire.games.count <= maximumGameCount else {
            throw TinyArcadeCatalogDecodeError.invalidDocument
        }
        let hostProfile: TinyArcadeCatalogHostProfileV1?
        if let profile = wire.hostProfile {
            guard profile.file == "host-profile-v1.tahost",
                  (56...UInt64(64 * 1_024)).contains(profile.length),
                  let hash = Self.hexData(profile.sha256), hash.count == 32,
                  let fileURL = URL(
                      string: profile.file,
                      relativeTo: cartridgeBaseURL
                  )?.absoluteURL,
                  Self.sameOrigin(fileURL, cartridgeBaseURL) else {
                throw TinyArcadeCatalogDecodeError.invalidDocument
            }
            hostProfile = TinyArcadeCatalogHostProfileV1(
                fileURL: fileURL,
                length: profile.length,
                sha256: hash
            )
        } else {
            hostProfile = nil
        }

        var seenGameIDs = Set<String>()
        var games: [TinyArcadeCatalogGameV1] = []
        games.reserveCapacity(wire.games.count)
        for (index, game) in wire.games.enumerated() {
            guard Self.canonicalIdentifier(game.gameID),
                  seenGameIDs.insert(game.gameID).inserted,
                  Self.validVersion(game.gameVersion),
                  game.abiVersion > 0,
                  game.stateVersion > 0,
                  (1...maximumCartridgeBytes).contains(game.wasmLength),
                  Self.validText(game.title, maximumUTF8Bytes: 256),
                  Self.validText(game.summary, maximumUTF8Bytes: 1_024),
                  Self.canonicalIdentifier(game.signingKeyID, maximumUTF8Bytes: 64),
                  let hash = Self.hexData(game.wasmSHA256), hash.count == 32,
                  let signature = Data(base64Encoded: game.signature),
                  signature.count == 64,
                  signature.base64EncodedString() == game.signature,
                  Self.validCartridgeFile(game.cartridge, version: game.gameVersion),
                  let cartridgeURL = URL(string: game.cartridge, relativeTo: cartridgeBaseURL)?.absoluteURL,
                  Self.sameOrigin(cartridgeURL, cartridgeBaseURL) else {
                throw TinyArcadeCatalogDecodeError.invalidEntry(index)
            }

            let wireLocalizations = game.localizations ?? [:]
            guard wireLocalizations.count <= 16 else {
                throw TinyArcadeCatalogDecodeError.invalidEntry(index)
            }
            var localizations: [String: TinyArcadeCatalogLocalizationV1] = [:]
            localizations.reserveCapacity(wireLocalizations.count)
            var seenLanguageTags = Set<String>()
            for (languageTag, value) in wireLocalizations {
                guard Self.validLanguageTag(languageTag),
                      seenLanguageTags.insert(languageTag.lowercased()).inserted,
                      Self.validText(value.title, maximumUTF8Bytes: 256),
                      Self.validText(value.summary, maximumUTF8Bytes: 1_024) else {
                    throw TinyArcadeCatalogDecodeError.invalidEntry(index)
                }
                localizations[languageTag] = TinyArcadeCatalogLocalizationV1(
                    title: value.title,
                    summary: value.summary
                )
            }

            games.append(
                TinyArcadeCatalogGameV1(
                    entry: TinyArcadeReviewedCatalogEntry(
                        gameID: game.gameID,
                        gameVersion: game.gameVersion,
                        abiVersion: game.abiVersion,
                        stateVersion: game.stateVersion,
                        wasmLength: game.wasmLength,
                        wasmSHA256: hash,
                        signingKeyID: game.signingKeyID,
                        signature: signature
                    ),
                    title: game.title,
                    summary: game.summary,
                    localizations: localizations,
                    cartridgeURL: cartridgeURL
                )
            )
        }
        return TinyArcadeCatalogV1(
            catalogID: wire.catalogID,
            hostProfile: hostProfile,
            games: games
        )
    }

    /// Resolves an exact `tinyarcade://game/<game-id>` selection. It performs
    /// no network, cache or runtime operation.
    public func game(forDeepLink url: URL, scheme: String = "tinyarcade") -> TinyArcadeCatalogGameV1? {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              components.scheme?.caseInsensitiveCompare(scheme) == .orderedSame,
              components.host?.caseInsensitiveCompare("game") == .orderedSame,
              components.user == nil,
              components.password == nil,
              components.port == nil,
              components.query == nil,
              components.fragment == nil else { return nil }
        let path = components.path.split(separator: "/", omittingEmptySubsequences: true)
        guard path.count == 1 else { return nil }
        let gameID = String(path[0])
        return games.first { $0.entry.gameID == gameID }
    }

    private init(
        catalogID: String,
        hostProfile: TinyArcadeCatalogHostProfileV1?,
        games: [TinyArcadeCatalogGameV1]
    ) {
        self.catalogID = catalogID
        self.hostProfile = hostProfile
        self.games = games
    }

    fileprivate static func validBaseURL(_ url: URL) -> Bool {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
            return false
        }
        return components.scheme == "https"
            && components.host != nil
            && components.user == nil
            && components.password == nil
            && components.query == nil
            && components.fragment == nil
            && url.hasDirectoryPath
    }

    fileprivate static func sameOrigin(_ lhs: URL, _ rhs: URL) -> Bool {
        lhs.scheme == rhs.scheme && lhs.host == rhs.host && lhs.port == rhs.port
    }

    private static func canonicalIdentifier(
        _ value: String,
        maximumUTF8Bytes: Int = 128
    ) -> Bool {
        let bytes = Array(value.utf8)
        return !bytes.isEmpty && bytes.count <= maximumUTF8Bytes && bytes.allSatisfy {
            (97...122).contains($0) || (48...57).contains($0) || [46, 95, 45].contains($0)
        }
    }

    private static func validVersion(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        return !bytes.isEmpty && bytes.count <= 64 && bytes.allSatisfy {
            (65...90).contains($0) || (97...122).contains($0)
                || (48...57).contains($0) || [46, 95, 43, 45].contains($0)
        }
    }

    private static func validText(_ value: String, maximumUTF8Bytes: Int) -> Bool {
        !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && value.utf8.count <= maximumUTF8Bytes
    }

    private static func validLanguageTag(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        return !bytes.isEmpty && bytes.count <= 35 && bytes.allSatisfy {
            (65...90).contains($0) || (97...122).contains($0)
                || (48...57).contains($0) || $0 == 45
        }
    }

    private static func validCartridgeFile(_ value: String, version: String) -> Bool {
        let bytes = Array(value.utf8)
        guard !bytes.isEmpty, bytes.count <= 160,
              value.hasSuffix("-\(version).wasm"),
              !value.hasPrefix("."),
              !value.contains("..") else { return false }
        return bytes.allSatisfy {
            (65...90).contains($0) || (97...122).contains($0)
                || (48...57).contains($0) || [46, 95, 43, 45].contains($0)
        }
    }

    private static func hexData(_ value: String) -> Data? {
        let bytes = Array(value.utf8)
        guard bytes.count == 64 else { return nil }
        var output = Data(capacity: 32)
        for index in stride(from: 0, to: bytes.count, by: 2) {
            guard let high = hexNibble(bytes[index]),
                  let low = hexNibble(bytes[index + 1]) else { return nil }
            output.append(high << 4 | low)
        }
        return output
    }

    private static func hexNibble(_ byte: UInt8) -> UInt8? {
        switch byte {
        case 48...57: return byte - 48
        case 97...102: return byte - 87
        default: return nil
        }
    }

    private struct WireCatalog: Decodable {
        let schemaVersion: UInt32
        let catalogID: String
        let hostProfile: WireHostProfile?
        let games: [WireGame]

        enum CodingKeys: String, CodingKey {
            case schemaVersion = "schema_version"
            case catalogID = "catalog_id"
            case hostProfile = "host_profile"
            case games
        }
    }

    private struct WireHostProfile: Decodable {
        let file: String
        let length: UInt64
        let sha256: String
    }

    private struct WireGame: Decodable {
        let gameID: String
        let gameVersion: String
        let title: String
        let summary: String
        let localizations: [String: WireLocalization]?
        let cartridge: String
        let abiVersion: UInt32
        let stateVersion: UInt32
        let wasmLength: UInt64
        let wasmSHA256: String
        let signingKeyID: String
        let signature: String

        enum CodingKeys: String, CodingKey {
            case gameID = "game_id"
            case gameVersion = "game_version"
            case title, summary, localizations, cartridge
            case abiVersion = "abi_version"
            case stateVersion = "state_version"
            case wasmLength = "wasm_length"
            case wasmSHA256 = "wasm_sha256"
            case signingKeyID = "signing_key_id"
            case signature
        }
    }

    private struct WireLocalization: Decodable {
        let title: String
        let summary: String
    }
}

public enum TinyArcadeHTTPError: Error, Equatable {
    case invalidURL
    case invalidResponse
    case httpStatus(Int)
    case redirectRejected
    case unsupportedContentType
    case responseTooLarge
    case lengthMismatch
    case hostProfileMismatch
    case requestQueueFull
    case cancelled
    case transportFailure
}

/// App-owned HTTPS transport for official discovery and cartridge objects.
/// Bytes are accumulated through URLSession delegate chunks and cancellation
/// occurs as soon as the declared or received length exceeds the caller's cap.
/// The guest and interpreter receive no network capability.
public final class TinyArcadeHTTPSClientV1: @unchecked Sendable {
    public let timeoutInterval: TimeInterval
    public let maximumConcurrentRequests: Int
    public let maximumQueuedRequests: Int
    private let configuration: URLSessionConfiguration
    private let requestGate: TinyArcadeHTTPRequestGate

    public convenience init(
        timeoutInterval: TimeInterval = 30,
        maximumConcurrentRequests: Int = 2,
        maximumQueuedRequests: Int = 16
    ) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.init(
            configuration: configuration,
            timeoutInterval: timeoutInterval,
            maximumConcurrentRequests: maximumConcurrentRequests,
            maximumQueuedRequests: maximumQueuedRequests
        )
    }

    init(
        configuration: URLSessionConfiguration,
        timeoutInterval: TimeInterval,
        maximumConcurrentRequests: Int = 2,
        maximumQueuedRequests: Int = 16
    ) {
        let requestLimit = min(4, max(1, maximumConcurrentRequests))
        let queueLimit = min(64, max(0, maximumQueuedRequests))
        let copied = configuration.copy() as? URLSessionConfiguration ?? configuration
        copied.httpMaximumConnectionsPerHost = requestLimit
        self.configuration = copied
        self.timeoutInterval = min(120, max(5, timeoutInterval))
        self.maximumConcurrentRequests = requestLimit
        self.maximumQueuedRequests = queueLimit
        requestGate = TinyArcadeHTTPRequestGate(
            activeLimit: requestLimit,
            queueLimit: queueLimit
        )
    }

    public func fetchCatalog(
        at catalogURL: URL,
        cartridgeBaseURL: URL,
        maximumCartridgeBytes: UInt64 = 8 * 1_024 * 1_024
    ) async throws -> TinyArcadeCatalogV1 {
        guard TinyArcadeCatalogV1.validBaseURL(cartridgeBaseURL),
              TinyArcadeCatalogV1.sameOrigin(catalogURL, cartridgeBaseURL) else {
            throw TinyArcadeHTTPError.invalidURL
        }
        let data = try await fetch(
            catalogURL,
            maximumBytes: TinyArcadeCatalogV1.maximumDocumentBytes,
            exactBytes: nil,
            contentTypes: ["application/json"]
        )
        return try TinyArcadeCatalogV1.decode(
            data,
            cartridgeBaseURL: cartridgeBaseURL,
            maximumCartridgeBytes: maximumCartridgeBytes
        )
    }

    public func fetchCartridge(_ game: TinyArcadeCatalogGameV1) async throws -> Data {
        guard game.entry.wasmLength <= UInt64(Int.max) else {
            throw TinyArcadeHTTPError.responseTooLarge
        }
        return try await fetch(
            game.cartridgeURL,
            maximumBytes: Int(game.entry.wasmLength),
            exactBytes: Int(game.entry.wasmLength),
            contentTypes: ["application/wasm", "application/octet-stream"]
        )
    }

    public func fetchHostProfile(
        _ profile: TinyArcadeCatalogHostProfileV1,
        matching expected: TinyArcadeHostProfileV1
    ) async throws -> Data {
        guard profile.length <= UInt64(Int.max),
              (56...UInt64(64 * 1_024)).contains(profile.length),
              profile.sha256.count == 32 else {
            throw TinyArcadeHTTPError.responseTooLarge
        }
        guard profile.length == UInt64(expected.encoded.count) else {
            throw TinyArcadeHTTPError.hostProfileMismatch
        }
        let data = try await fetch(
            profile.fileURL,
            maximumBytes: Int(profile.length),
            exactBytes: Int(profile.length),
            contentTypes: [
                "application/octet-stream",
                "application/vnd.tinyarcade.host-profile",
            ]
        )
        guard expected.matchesPublishedBytes(data) else {
            throw TinyArcadeHTTPError.hostProfileMismatch
        }
        return data
    }

    private func fetch(
        _ url: URL,
        maximumBytes: Int,
        exactBytes: Int?,
        contentTypes: Set<String>
    ) async throws -> Data {
        guard url.scheme == "https", url.host != nil,
              url.user == nil, url.password == nil,
              url.fragment == nil, maximumBytes > 0 else {
            throw TinyArcadeHTTPError.invalidURL
        }
        do {
            try await requestGate.acquire()
        } catch let error as TinyArcadeHTTPError {
            throw error
        } catch {
            throw TinyArcadeHTTPError.cancelled
        }
        do {
            let result = try await performFetch(
                url,
                maximumBytes: maximumBytes,
                exactBytes: exactBytes,
                contentTypes: contentTypes
            )
            await requestGate.release()
            return result
        } catch {
            await requestGate.release()
            throw error
        }
    }

    private func performFetch(
        _ url: URL,
        maximumBytes: Int,
        exactBytes: Int?,
        contentTypes: Set<String>
    ) async throws -> Data {
        var request = URLRequest(
            url: url,
            cachePolicy: .reloadIgnoringLocalCacheData,
            timeoutInterval: timeoutInterval
        )
        request.httpMethod = "GET"
        request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
        let transfer = TinyArcadeBoundedHTTPTransfer(
            configuration: configuration,
            request: request,
            maximumBytes: maximumBytes,
            exactBytes: exactBytes,
            contentTypes: contentTypes
        )
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation {
                (continuation: CheckedContinuation<Data, any Error>) in
                transfer.start(continuation)
            }
        } onCancel: {
            transfer.cancel()
        }
    }
}

private actor TinyArcadeHTTPRequestGate {
    private struct Waiter {
        let id: UUID
        let continuation: CheckedContinuation<Void, any Error>
    }

    private let activeLimit: Int
    private let queueLimit: Int
    private var active = 0
    private var waiters: [Waiter] = []

    init(activeLimit: Int, queueLimit: Int) {
        self.activeLimit = activeLimit
        self.queueLimit = queueLimit
    }

    func acquire() async throws {
        try Task.checkCancellation()
        if active < activeLimit {
            active += 1
            return
        }
        guard waiters.count < queueLimit else {
            throw TinyArcadeHTTPError.requestQueueFull
        }
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation {
                (continuation: CheckedContinuation<Void, any Error>) in
                if Task.isCancelled {
                    continuation.resume(throwing: TinyArcadeHTTPError.cancelled)
                } else {
                    waiters.append(Waiter(id: id, continuation: continuation))
                }
            }
        } onCancel: {
            Task { await self.cancelWaiter(id) }
        }
    }

    func release() {
        if waiters.isEmpty {
            active -= 1
        } else {
            waiters.removeFirst().continuation.resume()
        }
    }

    private func cancelWaiter(_ id: UUID) {
        guard let index = waiters.firstIndex(where: { $0.id == id }) else { return }
        waiters.remove(at: index).continuation.resume(throwing: TinyArcadeHTTPError.cancelled)
    }
}

private final class TinyArcadeBoundedHTTPTransfer: NSObject,
    URLSessionDataDelegate, URLSessionTaskDelegate, @unchecked Sendable {
    private let configuration: URLSessionConfiguration
    private let request: URLRequest
    private let maximumBytes: Int
    private let exactBytes: Int?
    private let contentTypes: Set<String>
    private let lock = NSLock()
    private let delegateQueue: OperationQueue = {
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        return queue
    }()
    private var continuation: CheckedContinuation<Data, any Error>?
    private var session: URLSession?
    private var task: URLSessionDataTask?
    private var data = Data()
    private var completed = false
    private var cancellationRequested = false

    init(
        configuration: URLSessionConfiguration,
        request: URLRequest,
        maximumBytes: Int,
        exactBytes: Int?,
        contentTypes: Set<String>
    ) {
        self.configuration = configuration
        self.request = request
        self.maximumBytes = maximumBytes
        self.exactBytes = exactBytes
        self.contentTypes = contentTypes
    }

    func start(_ continuation: CheckedContinuation<Data, any Error>) {
        lock.lock()
        if completed || cancellationRequested {
            completed = true
            lock.unlock()
            continuation.resume(throwing: TinyArcadeHTTPError.cancelled)
            return
        }
        self.continuation = continuation
        let session = URLSession(
            configuration: configuration,
            delegate: self,
            delegateQueue: delegateQueue
        )
        let task = session.dataTask(with: request)
        self.session = session
        self.task = task
        lock.unlock()
        task.resume()
    }

    func cancel() {
        lock.lock()
        cancellationRequested = true
        let task = self.task
        let hasContinuation = continuation != nil
        lock.unlock()
        task?.cancel()
        if hasContinuation { finish(.failure(TinyArcadeHTTPError.cancelled)) }
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        completionHandler(nil)
        finish(.failure(TinyArcadeHTTPError.redirectRejected))
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping @Sendable (URLSession.ResponseDisposition) -> Void
    ) {
        guard let response = response as? HTTPURLResponse else {
            completionHandler(.cancel)
            finish(.failure(TinyArcadeHTTPError.invalidResponse))
            return
        }
        guard response.statusCode == 200 else {
            completionHandler(.cancel)
            finish(.failure(TinyArcadeHTTPError.httpStatus(response.statusCode)))
            return
        }
        guard let mime = response.mimeType?.lowercased(), contentTypes.contains(mime) else {
            completionHandler(.cancel)
            finish(.failure(TinyArcadeHTTPError.unsupportedContentType))
            return
        }
        let declared = response.expectedContentLength
        if declared > Int64(maximumBytes) {
            completionHandler(.cancel)
            finish(.failure(TinyArcadeHTTPError.responseTooLarge))
            return
        }
        if let exactBytes, declared >= 0, declared != Int64(exactBytes) {
            completionHandler(.cancel)
            finish(.failure(TinyArcadeHTTPError.lengthMismatch))
            return
        }
        completionHandler(.allow)
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive chunk: Data
    ) {
        lock.lock()
        guard !completed else {
            lock.unlock()
            return
        }
        guard chunk.count <= maximumBytes - data.count else {
            let task = self.task
            lock.unlock()
            task?.cancel()
            finish(.failure(TinyArcadeHTTPError.responseTooLarge))
            return
        }
        data.append(chunk)
        lock.unlock()
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didCompleteWithError error: (any Error)?
    ) {
        if let error {
            let code = (error as NSError).code
            finish(
                .failure(
                    code == NSURLErrorCancelled
                        ? TinyArcadeHTTPError.cancelled
                        : TinyArcadeHTTPError.transportFailure
                )
            )
            return
        }
        lock.lock()
        let received = data
        lock.unlock()
        if let exactBytes, received.count != exactBytes {
            finish(.failure(TinyArcadeHTTPError.lengthMismatch))
        } else {
            finish(.success(received))
        }
    }

    private func finish(_ result: Result<Data, any Error>) {
        lock.lock()
        guard !completed else {
            lock.unlock()
            return
        }
        completed = true
        let continuation = self.continuation
        self.continuation = nil
        let session = self.session
        self.task = nil
        self.session = nil
        lock.unlock()
        session?.invalidateAndCancel()
        continuation?.resume(with: result)
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

/// Deterministic callback-free description of one exact app build's limits and
/// app-compiled native imports. Publish these bytes for converter preflight.
public struct TinyArcadeHostProfileV1: Sendable, Equatable {
    public let encoded: Data

    public func matchesPublishedBytes(_ data: Data) -> Bool {
        encoded == data
    }

    @MainActor
    public static func appBuild(
        nativeFunctions: [TinyArcadeNativeFunctionV1] = [],
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws -> Self {
        var config = tinyarcade_config_v1()
        try TinyArcadeRuntimeV1.check(tinyarcade_v1_default_config(&config))
        configure(&config)
        return try TinyArcadeRuntimeV1.withNativeFunctionTable(nativeFunctions) {
            table,
            count,
            _ in
            var required = 0
            let query = tinyarcade_v1_copy_host_profile(
                &config,
                table,
                count,
                nil,
                0,
                &required
            )
            guard query == TINYARCADE_BUFFER_TOO_SMALL,
                  (56...(64 * 1_024)).contains(required) else {
                try TinyArcadeRuntimeV1.check(query)
                throw TinyArcadeRuntimeError(
                    status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                    message: "invalid host profile length"
                )
            }
            var data = Data(count: required)
            let status = data.withUnsafeMutableBytes { output in
                tinyarcade_v1_copy_host_profile(
                    &config,
                    table,
                    count,
                    output.bindMemory(to: UInt8.self).baseAddress,
                    output.count,
                    &required
                )
            }
            try TinyArcadeRuntimeV1.check(status)
            guard required == data.count, data.prefix(4) == Data("TAH1".utf8) else {
                throw TinyArcadeRuntimeError(
                    status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                    message: "host profile length changed"
                )
            }
            return Self(encoded: data)
        }
    }

    /// Static compatibility only: this never instantiates the module or calls
    /// a native handler. Dynamic output/fuel conformance remains a later gate.
    @MainActor
    public func inspectCompatibleCartridge(
        _ cartridge: Data
    ) throws -> TinyArcadeCartridgeDescriptorV1 {
        let status = cartridge.withUnsafeBytes { wasm in
            encoded.withUnsafeBytes { profile in
                tinyarcade_v1_check_cartridge_host_profile(
                    wasm.bindMemory(to: UInt8.self).baseAddress,
                    wasm.count,
                    profile.bindMemory(to: UInt8.self).baseAddress,
                    profile.count
                )
            }
        }
        try TinyArcadeRuntimeV1.check(status)
        return try TinyArcadeCartridgeDescriptorV1.inspect(cartridge)
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

/// Main-actor owner for verified, content-addressed cartridge storage.
/// Network transfer remains app-owned; only complete downloaded bytes enter
/// `activate`, which verifies current trust before changing active state.
@MainActor
public final class TinyArcadeCartridgeCacheV1 {
    private var handle: OpaquePointer?

    public init(
        directoryURL: URL,
        maxWasmBytes: UInt64 = 8 * 1_024 * 1_024
    ) throws {
        guard directoryURL.isFileURL, maxWasmBytes > 0 else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "cartridge cache requires a file URL and positive byte limit"
            )
        }
        let path = Data(directoryURL.path.utf8)
        var opened: OpaquePointer?
        let status = path.withUnsafeBytes { bytes in
            tinyarcade_v1_cache_create(
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                maxWasmBytes,
                &opened
            )
        }
        try TinyArcadeRuntimeV1.check(status)
        guard let opened else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "runtime returned a null cartridge cache"
            )
        }
        handle = opened
    }

    isolated deinit {
        if let handle { _ = tinyarcade_v1_cache_close(handle) }
    }

    public func activate(
        entry: TinyArcadeReviewedCatalogEntry,
        cartridge: Data,
        trustStore: TinyArcadeTrustStoreV1
    ) throws {
        let handle = try liveHandle()
        let trust = try trustStore.liveHandle()
        let status = entry.withCEntry { cEntry in
            cartridge.withUnsafeBytes { bytes in
                tinyarcade_v1_cache_activate(
                    handle,
                    cEntry,
                    bytes.bindMemory(to: UInt8.self).baseAddress,
                    bytes.count,
                    trust
                )
            }
        }
        try TinyArcadeRuntimeV1.check(status)
    }

    public func loadActive(
        entry: TinyArcadeReviewedCatalogEntry,
        trustStore: TinyArcadeTrustStoreV1
    ) throws -> Data {
        let handle = try liveHandle()
        let trust = try trustStore.liveHandle()
        let status = entry.withCEntry {
            tinyarcade_v1_cache_load_active(handle, $0, trust)
        }
        try TinyArcadeRuntimeV1.check(status)
        return try copyWasm(handle)
    }

    public func rollback(
        to previousEntry: TinyArcadeReviewedCatalogEntry,
        trustStore: TinyArcadeTrustStoreV1
    ) throws -> Data {
        let handle = try liveHandle()
        let trust = try trustStore.liveHandle()
        let status = previousEntry.withCEntry {
            tinyarcade_v1_cache_rollback(handle, $0, trust)
        }
        try TinyArcadeRuntimeV1.check(status)
        return try copyWasm(handle)
    }

    public func close() throws {
        guard let handle else { return }
        try TinyArcadeRuntimeV1.check(tinyarcade_v1_cache_close(handle))
        self.handle = nil
    }

    private func liveHandle() throws -> OpaquePointer {
        guard let handle else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_INVALID_ARGUMENT.rawValue),
                message: "cartridge cache is closed"
            )
        }
        return handle
    }

    private func copyWasm(_ handle: OpaquePointer) throws -> Data {
        var count = 0
        let query = tinyarcade_v1_cache_copy_wasm(handle, nil, 0, &count)
        guard query == TINYARCADE_BUFFER_TOO_SMALL, count > 0 else {
            try TinyArcadeRuntimeV1.check(query)
            return Data()
        }
        var data = Data(count: count)
        let status = data.withUnsafeMutableBytes { bytes in
            tinyarcade_v1_cache_copy_wasm(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                &count
            )
        }
        try TinyArcadeRuntimeV1.check(status)
        return data
    }
}

public enum TinyArcadeReviewedLibraryError: Error, Equatable {
    case operationInProgress
}

/// Main-actor transaction that turns one reviewed catalog selection into a
/// ready runtime and only then makes it the cache's active generation.
///
/// The runtime preflight deliberately precedes cache activation. A cartridge
/// with a valid signature but unavailable native imports therefore cannot
/// replace the last playable active generation. Actor reentrancy across the
/// network await is closed with one explicit in-flight transaction.
@MainActor
public final class TinyArcadeReviewedLibraryV1 {
    private let transport: TinyArcadeHTTPSClientV1
    private let cache: TinyArcadeCartridgeCacheV1
    private let trustStore: TinyArcadeTrustStoreV1
    private let distributionPolicy: TinyArcadeDistributionPolicyV1
    private var installing = false

    public init(
        transport: TinyArcadeHTTPSClientV1,
        cache: TinyArcadeCartridgeCacheV1,
        trustStore: TinyArcadeTrustStoreV1,
        distributionPolicy: TinyArcadeDistributionPolicyV1 = .appStoreBundledOnly
    ) throws {
        try distributionPolicy.requireExternalCartridges()
        self.transport = transport
        self.cache = cache
        self.trustStore = trustStore
        self.distributionPolicy = distributionPolicy
    }

    public func fetchCatalog(
        at catalogURL: URL,
        cartridgeBaseURL: URL,
        maximumCartridgeBytes: UInt64 = 8 * 1_024 * 1_024
    ) async throws -> TinyArcadeCatalogV1 {
        try await transport.fetchCatalog(
            at: catalogURL,
            cartridgeBaseURL: cartridgeBaseURL,
            maximumCartridgeBytes: maximumCartridgeBytes
        )
    }

    public func installAndOpen(
        _ game: TinyArcadeCatalogGameV1,
        nativeFunctions: [TinyArcadeNativeFunctionV1] = [],
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) async throws -> TinyArcadeRuntimeV1 {
        guard !installing else { throw TinyArcadeReviewedLibraryError.operationInProgress }
        installing = true
        defer { installing = false }

        let cartridge = try await transport.fetchCartridge(game)
        try Task.checkCancellation()
        let runtime = try TinyArcadeRuntimeV1(
            reviewedCartridge: cartridge,
            entry: game.entry,
            trustStore: trustStore,
            nativeFunctions: nativeFunctions,
            distributionPolicy: distributionPolicy,
            configure: configure
        )
        do {
            try Task.checkCancellation()
            try cache.activate(
                entry: game.entry,
                cartridge: cartridge,
                trustStore: trustStore
            )
            return runtime
        } catch {
            try? runtime.close()
            throw error
        }
    }

    public func openActive(
        _ game: TinyArcadeCatalogGameV1,
        nativeFunctions: [TinyArcadeNativeFunctionV1] = [],
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws -> TinyArcadeRuntimeV1 {
        let cartridge = try cache.loadActive(entry: game.entry, trustStore: trustStore)
        return try TinyArcadeRuntimeV1(
            reviewedCartridge: cartridge,
            entry: game.entry,
            trustStore: trustStore,
            nativeFunctions: nativeFunctions,
            distributionPolicy: distributionPolicy,
            configure: configure
        )
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
        distributionPolicy: TinyArcadeDistributionPolicyV1 = .appStoreBundledOnly,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        try distributionPolicy.requireExternalCartridges()
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
        distributionPolicy: TinyArcadeDistributionPolicyV1 = .appStoreBundledOnly,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws {
        try distributionPolicy.requireExternalCartridges()
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
        let trust = try trustStore.liveHandle()
        let status = try entry.withCEntry { cEntry in
            try cartridge.withUnsafeBytes { cartridgeBytes in
                if nativeFunctions.isEmpty {
                    return tinyarcade_v1_open_reviewed(
                        cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                        cartridgeBytes.count,
                        cEntry,
                        trust,
                        &config,
                        &opened
                    )
                }
                return try Self.withNativeFunctionTable(nativeFunctions) { table, count, boxes in
                    let result = tinyarcade_v1_open_reviewed_with_native_modules(
                        cartridgeBytes.bindMemory(to: UInt8.self).baseAddress,
                        cartridgeBytes.count,
                        cEntry,
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

    /// Begins a deterministic replay at the runtime's current state. Subsequent
    /// ordinary `tick`/`tickMedia` calls are recorded until finish or cancel.
    public func beginReplayRecording() throws {
        try Self.check(tinyarcade_v1_replay_begin(try liveHandle()))
    }

    /// Discards an active recording or the last completed replay. It does not
    /// rewind gameplay state.
    public func cancelReplayRecording() throws {
        try Self.check(tinyarcade_v1_replay_cancel(try liveHandle()))
    }

    /// Completes the active recording and returns one canonical bounded
    /// `.tareplay` artifact suitable for `Data.write` or app-owned upload.
    public func finishReplayRecording() throws -> Data {
        let handle = try liveHandle()
        try Self.check(tinyarcade_v1_replay_finish(handle))
        return try copy(handle, tinyarcade_v1_copy_replay)
    }

    /// Restores and executes a replay against this runtime's exact loaded
    /// cartridge. Verification consumes this runtime's gameplay state; use a
    /// disposable fresh runtime when the current session must be preserved.
    @discardableResult
    public func verifyReplay(_ replay: Data) throws -> UInt32 {
        let handle = try liveHandle()
        var steps: UInt32 = 0
        let status = replay.withUnsafeBytes { bytes in
            tinyarcade_v1_replay_check(
                handle,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                bytes.count,
                &steps
            )
        }
        try Self.check(status)
        return steps
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

    /// Returns init stats immediately after open, then the most recent
    /// tick/suspend/resume attempt. A guest trap still updates this record.
    public func lastExecutionStats() throws -> TinyArcadeExecutionStatsV1 {
        var raw = tinyarcade_execution_stats_v1()
        try Self.check(tinyarcade_v1_last_execution_stats(try liveHandle(), &raw))
        guard raw.struct_size == UInt32(MemoryLayout<tinyarcade_execution_stats_v1>.size),
              let lifecycle = TinyArcadeGameLifecycleV1(rawValue: raw.lifecycle) else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                message: "runtime returned invalid execution stats"
            )
        }
        return TinyArcadeExecutionStatsV1(
            lifecycle: lifecycle,
            wasmSteps: raw.wasm_steps,
            memoryPages: raw.memory_pages,
            tableElements: raw.table_elements,
            nativeCalls: raw.native_calls,
            renderBytes: raw.render_bytes,
            audioBytes: raw.audio_bytes,
            stateBytes: raw.state_bytes
        )
    }

    /// Extended ABI v1.9 stats. The original method remains byte-for-byte
    /// compatible with v1.8 callers and intentionally omits these new fields.
    public func lastExecutionStatsV2() throws -> TinyArcadeExecutionStatsV2 {
        var raw = tinyarcade_execution_stats_v2()
        try Self.check(tinyarcade_v1_last_execution_stats_v2(try liveHandle(), &raw))
        guard raw.struct_size == UInt32(MemoryLayout<tinyarcade_execution_stats_v2>.size),
              let lifecycle = TinyArcadeGameLifecycleV1(rawValue: raw.lifecycle) else {
            throw TinyArcadeRuntimeError(
                status: Int32(TINYARCADE_DECODE_ERROR.rawValue),
                message: "runtime returned invalid extended execution stats"
            )
        }
        return TinyArcadeExecutionStatsV2(
            lifecycle: lifecycle,
            wasmSteps: raw.wasm_steps,
            peakCallDepth: raw.peak_call_depth,
            peakActivationSlots: raw.peak_activation_slots,
            memoryPages: raw.memory_pages,
            tableElements: raw.table_elements,
            nativeCalls: raw.native_calls,
            renderBytes: raw.render_bytes,
            audioBytes: raw.audio_bytes,
            stateBytes: raw.state_bytes
        )
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

    fileprivate static func withNativeFunctionTable<T>(
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

public enum TinyArcadeSnapshotStoreError: Error, Equatable {
    case invalidDirectory
    case invalidLimit
    case invalidGameID
    case unsafeStoredFile
    case storageFailure
}

public enum TinyArcadeSnapshotRestoreDispositionV1: Sendable, Equatable {
    case fresh
    case restored
    case discardedInvalid
}

public struct TinyArcadeSnapshotSessionV1 {
    public let runtime: TinyArcadeRuntimeV1
    public let gameClockMilliseconds: UInt32
    public let disposition: TinyArcadeSnapshotRestoreDispositionV1
}

/// Main-actor, per-game persistence for cartridge snapshots and the host-owned
/// game clock. The file envelope is bounded and checksummed; the embedded
/// runtime snapshot remains the authority for game id, ABI and state schema.
@MainActor
public final class TinyArcadeSnapshotStoreV1 {
    public let directoryURL: URL
    public let maximumSnapshotBytes: Int

    public init(
        directoryURL: URL,
        maximumSnapshotBytes: Int = 512 * 1_024
    ) throws {
        guard directoryURL.isFileURL else { throw TinyArcadeSnapshotStoreError.invalidDirectory }
        guard (1...(8 * 1_024 * 1_024)).contains(maximumSnapshotBytes) else {
            throw TinyArcadeSnapshotStoreError.invalidLimit
        }
        do {
            try FileManager.default.createDirectory(
                at: directoryURL,
                withIntermediateDirectories: true
            )
            let values = try directoryURL.resourceValues(
                forKeys: [.isDirectoryKey, .isSymbolicLinkKey]
            )
            guard values.isDirectory == true, values.isSymbolicLink != true else {
                throw TinyArcadeSnapshotStoreError.invalidDirectory
            }
            var backup = URLResourceValues()
            backup.isExcludedFromBackup = true
            var mutableDirectory = directoryURL
            try? mutableDirectory.setResourceValues(backup)
        } catch let error as TinyArcadeSnapshotStoreError {
            throw error
        } catch {
            throw TinyArcadeSnapshotStoreError.storageFailure
        }
        self.directoryURL = directoryURL
        self.maximumSnapshotBytes = maximumSnapshotBytes
    }

    public func openSession(
        makeRuntime: () throws -> TinyArcadeRuntimeV1
    ) throws -> TinyArcadeSnapshotSessionV1 {
        let candidate = try makeRuntime()
        let gameID = try candidate.gameID()
        let loaded: LoadedSnapshot
        do {
            loaded = try load(gameID: gameID)
        } catch {
            try? candidate.close()
            throw error
        }
        switch loaded {
        case .absent:
            return session(candidate, clock: 0, disposition: .fresh)
        case .invalid:
            try discard(gameID: gameID)
            return session(candidate, clock: 0, disposition: .discardedInvalid)
        case let .valid(clock, snapshot):
            do {
                try candidate.resume(snapshot: snapshot)
                return session(candidate, clock: clock, disposition: .restored)
            } catch {
                try? candidate.close()
                try discard(gameID: gameID)
                return session(try makeRuntime(), clock: 0, disposition: .discardedInvalid)
            }
        }
    }

    public func save(
        runtime: TinyArcadeRuntimeV1,
        gameClockMilliseconds: UInt32
    ) throws {
        let gameID = try runtime.gameID()
        guard Self.validGameID(gameID) else { throw TinyArcadeSnapshotStoreError.invalidGameID }
        let snapshot = try runtime.suspend()
        guard !snapshot.isEmpty, snapshot.count <= maximumSnapshotBytes else {
            throw TinyArcadeSnapshotStoreError.invalidLimit
        }
        let url = try fileURL(gameID: gameID)
        try rejectUnsafeExistingFile(url)
        var data = Data("TAS1".utf8)
        Self.append(UInt16(1), to: &data)
        Self.append(UInt16(32), to: &data)
        Self.append(gameClockMilliseconds, to: &data)
        Self.append(UInt16(gameID.utf8.count), to: &data)
        Self.append(UInt16(0), to: &data)
        Self.append(UInt32(snapshot.count), to: &data)
        Self.append(UInt32(0), to: &data)
        Self.append(UInt64(0), to: &data)
        data.append(contentsOf: gameID.utf8)
        data.append(snapshot)
        let checksum = Self.checksum(data)
        for offset in 0..<4 { data[20 + offset] = UInt8(truncatingIfNeeded: checksum >> (offset * 8)) }
        do {
            try data.write(to: url, options: .atomic)
            try FileManager.default.setAttributes(
                [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
                ofItemAtPath: url.path
            )
        } catch {
            throw TinyArcadeSnapshotStoreError.storageFailure
        }
    }

    public func discard(gameID: String) throws {
        let url = try fileURL(gameID: gameID)
        try rejectUnsafeExistingFile(url)
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            try FileManager.default.removeItem(at: url)
        } catch {
            throw TinyArcadeSnapshotStoreError.storageFailure
        }
    }

    private enum LoadedSnapshot {
        case absent
        case invalid
        case valid(clock: UInt32, snapshot: Data)
    }

    private func load(gameID: String) throws -> LoadedSnapshot {
        let url = try fileURL(gameID: gameID)
        try rejectUnsafeExistingFile(url)
        guard FileManager.default.fileExists(atPath: url.path) else { return .absent }
        do {
            let values = try url.resourceValues(forKeys: [.fileSizeKey])
            guard let size = values.fileSize,
                  (32...(32 + 128 + maximumSnapshotBytes)).contains(size) else {
                return .invalid
            }
            let data = try Data(contentsOf: url, options: .mappedIfSafe)
            guard data.count == size,
                  data.prefix(4) == Data("TAS1".utf8),
                  Self.u16(data, 4) == 1,
                  Self.u16(data, 6) == 32,
                  Self.u16(data, 14) == 0,
                  Self.u64(data, 24) == 0 else { return .invalid }
            let clock = Self.u32(data, 8)
            let idLength = Int(Self.u16(data, 12))
            let snapshotLength = Int(Self.u32(data, 16))
            guard (1...128).contains(idLength),
                  (1...maximumSnapshotBytes).contains(snapshotLength),
                  data.count == 32 + idLength + snapshotLength,
                  Self.u32(data, 20) == Self.checksum(data),
                  let storedID = String(data: data.subdata(in: 32..<(32 + idLength)), encoding: .utf8),
                  storedID == gameID else { return .invalid }
            return .valid(
                clock: clock,
                snapshot: data.subdata(in: (32 + idLength)..<data.count)
            )
        } catch let error as TinyArcadeSnapshotStoreError {
            throw error
        } catch {
            throw TinyArcadeSnapshotStoreError.storageFailure
        }
    }

    private func fileURL(gameID: String) throws -> URL {
        guard Self.validGameID(gameID) else { throw TinyArcadeSnapshotStoreError.invalidGameID }
        return directoryURL.appendingPathComponent("\(gameID).snapshot-v1", isDirectory: false)
    }

    private func rejectUnsafeExistingFile(_ url: URL) throws {
        if (try? FileManager.default.destinationOfSymbolicLink(atPath: url.path)) != nil {
            throw TinyArcadeSnapshotStoreError.unsafeStoredFile
        }
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true else {
                throw TinyArcadeSnapshotStoreError.unsafeStoredFile
            }
        } catch let error as TinyArcadeSnapshotStoreError {
            throw error
        } catch {
            throw TinyArcadeSnapshotStoreError.storageFailure
        }
    }

    private func session(
        _ runtime: TinyArcadeRuntimeV1,
        clock: UInt32,
        disposition: TinyArcadeSnapshotRestoreDispositionV1
    ) -> TinyArcadeSnapshotSessionV1 {
        TinyArcadeSnapshotSessionV1(
            runtime: runtime,
            gameClockMilliseconds: clock,
            disposition: disposition
        )
    }

    private static func validGameID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 128 && value.utf8.allSatisfy {
            (97...122).contains($0) || (48...57).contains($0) || [46, 95, 45].contains($0)
        }
    }

    private static func checksum(_ data: Data) -> UInt32 {
        var crc = UInt32.max
        for (offset, stored) in data.enumerated() {
            let byte: UInt8 = (20..<24).contains(offset) ? 0 : stored
            crc ^= UInt32(byte)
            for _ in 0..<8 { crc = (crc >> 1) ^ (0xedb8_8320 & (0 &- (crc & 1))) }
        }
        return ~crc
    }

    private static func append<T: FixedWidthInteger>(_ value: T, to data: inout Data) {
        withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) }
    }

    private static func u16(_ data: Data, _ offset: Int) -> UInt16 {
        UInt16(data[offset]) | UInt16(data[offset + 1]) << 8
    }

    private static func u32(_ data: Data, _ offset: Int) -> UInt32 {
        UInt32(data[offset]) | UInt32(data[offset + 1]) << 8
            | UInt32(data[offset + 2]) << 16 | UInt32(data[offset + 3]) << 24
    }

    private static func u64(_ data: Data, _ offset: Int) -> UInt64 {
        UInt64(u32(data, offset)) | UInt64(u32(data, offset + 4)) << 32
    }
}

public struct TinyArcadeButtonsV1: OptionSet, Sendable {
    public let rawValue: UInt32

    public init(rawValue: UInt32) {
        self.rawValue = rawValue
    }

    public static let left = Self(rawValue: 1 << 0)
    public static let right = Self(rawValue: 1 << 1)
    public static let up = Self(rawValue: 1 << 2)
    public static let down = Self(rawValue: 1 << 3)
    public static let primary = Self(rawValue: 1 << 4)
    public static let secondary = Self(rawValue: 1 << 5)
    public static let tertiary = Self(rawValue: 1 << 6)
    public static let start = Self(rawValue: 1 << 7)
    public static let menu = Self(rawValue: 1 << 8)
    public static let allKnown = Self(rawValue: (1 << 9) - 1)
}

public enum TinyArcadeInputStateError: Error, Equatable {
    case unknownButtons
    case tooManySources
}

/// Bounded multi-source input aggregation for touch controls, keyboards and
/// game controllers. Each source replaces its own complete pressed set, so
/// overlapping devices cannot release a button still held by another source.
public struct TinyArcadeInputStateV1: Sendable {
    public static let maximumSourceCount = 32

    public private(set) var buttons: TinyArcadeButtonsV1 = []
    private var sources: [UInt64: TinyArcadeButtonsV1] = [:]

    public init() {}

    public mutating func set(
        _ pressed: TinyArcadeButtonsV1,
        forSource source: UInt64
    ) throws {
        guard pressed.isSubset(of: .allKnown) else {
            throw TinyArcadeInputStateError.unknownButtons
        }
        if pressed.isEmpty {
            sources.removeValue(forKey: source)
        } else {
            guard sources[source] != nil || sources.count < Self.maximumSourceCount else {
                throw TinyArcadeInputStateError.tooManySources
            }
            sources[source] = pressed
        }
        buttons = sources.values.reduce([]) { $0.union($1) }
    }

    public mutating func releaseAll() {
        sources.removeAll(keepingCapacity: true)
        buttons = []
    }
}

public enum TinyArcadeFramePacerError: Error, Sendable, Equatable {
    case invalidMaximumFrameAdvance
    case invalidTimestamp
    case timestampWentBackwards
    case frameAdvanceTooLarge
}

/// Converts a monotonic foreground timestamp into bounded integer game-time
/// deltas without losing sub-millisecond remainder. Use CADisplayLink.timestamp
/// or another monotonic source, never Date/wall-clock time. Reset after a pause.
public struct TinyArcadeFramePacerV1: Sendable {
    public let maximumFrameAdvanceMilliseconds: UInt32
    private var previousTimestampSeconds: TimeInterval?
    private var fractionalMilliseconds = 0.0

    public init(
        maximumFrameAdvanceMilliseconds: UInt32 =
            TinyArcadeGameSessionV1.defaultMaximumFrameAdvanceMilliseconds
    ) throws {
        guard (1...1_000).contains(maximumFrameAdvanceMilliseconds) else {
            throw TinyArcadeFramePacerError.invalidMaximumFrameAdvance
        }
        self.maximumFrameAdvanceMilliseconds = maximumFrameAdvanceMilliseconds
    }

    /// The first timestamp after initialization or reset emits zero elapsed
    /// time. Rejected samples do not mutate the pacing baseline.
    public mutating func elapsedMilliseconds(
        at timestampSeconds: TimeInterval
    ) throws -> UInt32 {
        guard timestampSeconds.isFinite, timestampSeconds >= 0 else {
            throw TinyArcadeFramePacerError.invalidTimestamp
        }
        guard let previousTimestampSeconds else {
            self.previousTimestampSeconds = timestampSeconds
            fractionalMilliseconds = 0
            return 0
        }
        guard timestampSeconds >= previousTimestampSeconds else {
            throw TinyArcadeFramePacerError.timestampWentBackwards
        }
        let elapsed = (timestampSeconds - previousTimestampSeconds) * 1_000
            + fractionalMilliseconds
        guard elapsed.isFinite else { throw TinyArcadeFramePacerError.invalidTimestamp }
        guard elapsed <= Double(maximumFrameAdvanceMilliseconds) else {
            throw TinyArcadeFramePacerError.frameAdvanceTooLarge
        }
        let wholeMilliseconds = elapsed.rounded(.down)
        self.previousTimestampSeconds = timestampSeconds
        fractionalMilliseconds = elapsed - wholeMilliseconds
        return UInt32(wholeMilliseconds)
    }

    public mutating func reset() {
        previousTimestampSeconds = nil
        fractionalMilliseconds = 0
    }
}

public enum TinyArcadeGameSessionError: Error, Equatable {
    case invalidMaximumFrameAdvance
    case frameAdvanceTooLarge
    case clockExhausted
    case inactive
    case failed
    case closed
}

/// Main-actor gameplay owner for one runtime, monotonic game clock and all
/// input sources. The app supplies foreground elapsed time; background time is
/// excluded by simply not ticking and releasing inputs before persistence.
@MainActor
public final class TinyArcadeGameSessionV1 {
    public nonisolated static let defaultMaximumFrameAdvanceMilliseconds: UInt32 = 250

    public private(set) var gameClockMilliseconds: UInt32
    public private(set) var input = TinyArcadeInputStateV1()
    public private(set) var isActive = true
    public private(set) var isFailed = false
    public let maximumFrameAdvanceMilliseconds: UInt32
    private var runtime: TinyArcadeRuntimeV1?

    public init(
        runtime: TinyArcadeRuntimeV1,
        gameClockMilliseconds: UInt32 = 0,
        maximumFrameAdvanceMilliseconds: UInt32 = defaultMaximumFrameAdvanceMilliseconds
    ) throws {
        guard (1...1_000).contains(maximumFrameAdvanceMilliseconds) else {
            throw TinyArcadeGameSessionError.invalidMaximumFrameAdvance
        }
        self.runtime = runtime
        self.gameClockMilliseconds = gameClockMilliseconds
        self.maximumFrameAdvanceMilliseconds = maximumFrameAdvanceMilliseconds
    }

    public convenience init(
        restored session: TinyArcadeSnapshotSessionV1,
        maximumFrameAdvanceMilliseconds: UInt32 = defaultMaximumFrameAdvanceMilliseconds
    ) throws {
        try self.init(
            runtime: session.runtime,
            gameClockMilliseconds: session.gameClockMilliseconds,
            maximumFrameAdvanceMilliseconds: maximumFrameAdvanceMilliseconds
        )
    }

    public func setButtons(
        _ pressed: TinyArcadeButtonsV1,
        forSource source: UInt64
    ) throws {
        _ = try liveRuntime()
        guard !isFailed else { throw TinyArcadeGameSessionError.failed }
        guard isActive else { throw TinyArcadeGameSessionError.inactive }
        try input.set(pressed, forSource: source)
    }

    public func releaseAllInputs() {
        input.releaseAll()
    }

    public func tick(elapsedMilliseconds: UInt32) throws -> TinyArcadeMediaFrame {
        let runtime = try liveRuntime()
        guard !isFailed else { throw TinyArcadeGameSessionError.failed }
        guard isActive else { throw TinyArcadeGameSessionError.inactive }
        guard elapsedMilliseconds <= maximumFrameAdvanceMilliseconds else {
            throw TinyArcadeGameSessionError.frameAdvanceTooLarge
        }
        let (nextClock, overflow) = gameClockMilliseconds.addingReportingOverflow(
            elapsedMilliseconds
        )
        guard !overflow else { throw TinyArcadeGameSessionError.clockExhausted }
        do {
            let frame = try runtime.tickMedia(
                buttons: input.buttons.rawValue,
                clockMilliseconds: nextClock
            )
            gameClockMilliseconds = nextClock
            return frame
        } catch {
            isFailed = true
            throw error
        }
    }

    public func save(to store: TinyArcadeSnapshotStoreV1) throws {
        let runtime = try liveRuntime()
        guard !isFailed else { throw TinyArcadeGameSessionError.failed }
        do {
            try store.save(
                runtime: runtime,
                gameClockMilliseconds: gameClockMilliseconds
            )
        } catch let error as TinyArcadeRuntimeError {
            isFailed = true
            throw error
        }
    }

    /// Release held controls, make ticks impossible, then persist the exact
    /// last successful game clock. The session stays inactive if saving fails.
    public func deactivateAndSave(to store: TinyArcadeSnapshotStoreV1) throws {
        _ = try liveRuntime()
        guard !isFailed else { throw TinyArcadeGameSessionError.failed }
        releaseAllInputs()
        isActive = false
        try save(to: store)
    }

    /// Resume foreground ticking. Reset the app's frame pacer before supplying
    /// the first new timestamp so background elapsed time is not included.
    public func activate() throws {
        _ = try liveRuntime()
        guard !isFailed else { throw TinyArcadeGameSessionError.failed }
        releaseAllInputs()
        isActive = true
    }

    public func close() throws {
        guard let runtime else { return }
        releaseAllInputs()
        isActive = false
        try runtime.close()
        self.runtime = nil
    }

    private func liveRuntime() throws -> TinyArcadeRuntimeV1 {
        guard let runtime else { throw TinyArcadeGameSessionError.closed }
        return runtime
    }
}

public enum TinyArcadePrivateLibraryError: Error, Equatable {
    case invalidDirectory
    case invalidLimit
    case invalidIdentity
    case unsafeStoredFile
    case cartridgeNotFound
    case tooManyCartridges
    case unsupportedNativeCapabilities([String])
    case storageFailure
}

public struct TinyArcadePrivateCartridgeV1: Sendable, Equatable {
    public let gameID: String
    public let gameVersion: String
    public let fileURL: URL

    fileprivate init(gameID: String, gameVersion: String, fileURL: URL) {
        self.gameID = gameID
        self.gameVersion = gameVersion
        self.fileURL = fileURL
    }
}

/// Main-actor storage for cartridges explicitly imported into the user's own
/// library. Import always preflights the private core-only runtime before one
/// atomic version replacement. This owner does not download, publish, sign or
/// grant a native capability.
@MainActor
public final class TinyArcadePrivateLibraryV1 {
    public nonisolated static let maximumCartridgeCount = 256
    public nonisolated static let runtimeMaximumCartridgeBytes = 2 * 1_024 * 1_024

    public let directoryURL: URL
    public let maximumCartridgeBytes: Int
    private let distributionPolicy: TinyArcadeDistributionPolicyV1

    public init(
        directoryURL: URL,
        maximumCartridgeBytes: Int = runtimeMaximumCartridgeBytes,
        distributionPolicy: TinyArcadeDistributionPolicyV1 = .appStoreBundledOnly
    ) throws {
        try distributionPolicy.requireExternalCartridges()
        guard directoryURL.isFileURL else {
            throw TinyArcadePrivateLibraryError.invalidDirectory
        }
        guard (1...Self.runtimeMaximumCartridgeBytes).contains(maximumCartridgeBytes) else {
            throw TinyArcadePrivateLibraryError.invalidLimit
        }
        do {
            try FileManager.default.createDirectory(
                at: directoryURL,
                withIntermediateDirectories: true
            )
            let values = try directoryURL.resourceValues(
                forKeys: [.isDirectoryKey, .isSymbolicLinkKey]
            )
            guard values.isDirectory == true, values.isSymbolicLink != true else {
                throw TinyArcadePrivateLibraryError.invalidDirectory
            }
            var backup = URLResourceValues()
            backup.isExcludedFromBackup = true
            var mutableDirectory = directoryURL
            try? mutableDirectory.setResourceValues(backup)
        } catch let error as TinyArcadePrivateLibraryError {
            throw error
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
        self.directoryURL = directoryURL
        self.maximumCartridgeBytes = maximumCartridgeBytes
        self.distributionPolicy = distributionPolicy
    }

    /// Preflights exact bytes under the private core-only policy, then atomically
    /// installs or replaces that exact game-id/version slot.
    public func importCartridge(
        _ cartridge: Data,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws -> TinyArcadePrivateCartridgeV1 {
        guard !cartridge.isEmpty, cartridge.count <= maximumCartridgeBytes else {
            throw TinyArcadePrivateLibraryError.invalidLimit
        }
        let descriptor = try TinyArcadeCartridgeDescriptorV1.inspect(cartridge)
        guard descriptor.nativeCapabilities.isEmpty else {
            throw TinyArcadePrivateLibraryError.unsupportedNativeCapabilities(
                descriptor.nativeCapabilities
            )
        }
        let runtime = try TinyArcadeRuntimeV1(
            privateCartridge: cartridge,
            distributionPolicy: distributionPolicy,
            configure: configure
        )
        do {
            guard try runtime.gameID() == descriptor.gameID,
                  try runtime.gameVersion() == descriptor.gameVersion else {
                throw TinyArcadePrivateLibraryError.invalidIdentity
            }
            try runtime.close()
        } catch {
            try? runtime.close()
            throw error
        }
        let item = try self.cartridge(
            gameID: descriptor.gameID,
            gameVersion: descriptor.gameVersion
        )
        try rejectUnsafeExistingFile(item.fileURL)
        try ensureCapacity(for: item.fileURL)
        do {
            try cartridge.write(to: item.fileURL, options: .atomic)
            try FileManager.default.setAttributes(
                [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
                ofItemAtPath: item.fileURL.path
            )
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
        return item
    }

    /// Enumerates bounded canonical slots without executing their guest code.
    /// Each item is fully revalidated when `open` constructs its runtime.
    public func installedCartridges() throws -> [TinyArcadePrivateCartridgeV1] {
        do {
            let urls = try FileManager.default.contentsOfDirectory(
                at: directoryURL,
                includingPropertiesForKeys: [
                    .isRegularFileKey,
                    .isSymbolicLinkKey,
                    .fileSizeKey,
                ],
                options: [.skipsHiddenFiles]
            ).filter { $0.pathExtension == "wasm" }
            guard urls.count <= Self.maximumCartridgeCount else {
                throw TinyArcadePrivateLibraryError.tooManyCartridges
            }
            var items: [TinyArcadePrivateCartridgeV1] = []
            items.reserveCapacity(urls.count)
            for url in urls {
                try rejectUnsafeExistingFile(url)
                let values = try url.resourceValues(forKeys: [.fileSizeKey])
                guard let size = values.fileSize,
                      (1...maximumCartridgeBytes).contains(size),
                      let identity = Self.identity(from: url.lastPathComponent) else {
                    throw TinyArcadePrivateLibraryError.unsafeStoredFile
                }
                items.append(
                    TinyArcadePrivateCartridgeV1(
                        gameID: identity.gameID,
                        gameVersion: identity.gameVersion,
                        fileURL: url
                    )
                )
            }
            return items.sorted {
                $0.gameID == $1.gameID
                    ? $0.gameVersion < $1.gameVersion
                    : $0.gameID < $1.gameID
            }
        } catch let error as TinyArcadePrivateLibraryError {
            throw error
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
    }

    public func open(
        _ item: TinyArcadePrivateCartridgeV1,
        configure: (inout tinyarcade_config_v1) -> Void = { _ in }
    ) throws -> TinyArcadeRuntimeV1 {
        let expected = try cartridge(gameID: item.gameID, gameVersion: item.gameVersion)
        guard expected.fileURL.standardizedFileURL == item.fileURL.standardizedFileURL else {
            throw TinyArcadePrivateLibraryError.invalidIdentity
        }
        let bytes = try load(expected)
        let runtime = try TinyArcadeRuntimeV1(
            privateCartridge: bytes,
            distributionPolicy: distributionPolicy,
            configure: configure
        )
        do {
            guard try runtime.gameID() == item.gameID,
                  try runtime.gameVersion() == item.gameVersion else {
                throw TinyArcadePrivateLibraryError.invalidIdentity
            }
            return runtime
        } catch {
            try? runtime.close()
            throw error
        }
    }

    public func remove(_ item: TinyArcadePrivateCartridgeV1) throws {
        let expected = try cartridge(gameID: item.gameID, gameVersion: item.gameVersion)
        guard expected.fileURL.standardizedFileURL == item.fileURL.standardizedFileURL else {
            throw TinyArcadePrivateLibraryError.invalidIdentity
        }
        guard FileManager.default.fileExists(atPath: expected.fileURL.path) else { return }
        try rejectUnsafeExistingFile(expected.fileURL)
        do {
            try FileManager.default.removeItem(at: expected.fileURL)
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
    }

    private func load(_ item: TinyArcadePrivateCartridgeV1) throws -> Data {
        guard FileManager.default.fileExists(atPath: item.fileURL.path) else {
            throw TinyArcadePrivateLibraryError.cartridgeNotFound
        }
        try rejectUnsafeExistingFile(item.fileURL)
        do {
            let values = try item.fileURL.resourceValues(forKeys: [.fileSizeKey])
            guard let size = values.fileSize,
                  (1...maximumCartridgeBytes).contains(size) else {
                throw TinyArcadePrivateLibraryError.invalidLimit
            }
            let bytes = try Data(contentsOf: item.fileURL, options: .mappedIfSafe)
            guard bytes.count == size else {
                throw TinyArcadePrivateLibraryError.storageFailure
            }
            return bytes
        } catch let error as TinyArcadePrivateLibraryError {
            throw error
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
    }

    private func cartridge(
        gameID: String,
        gameVersion: String
    ) throws -> TinyArcadePrivateCartridgeV1 {
        guard Self.validGameID(gameID), Self.validVersion(gameVersion) else {
            throw TinyArcadePrivateLibraryError.invalidIdentity
        }
        let leaf = "\(gameID)@\(gameVersion).wasm"
        return TinyArcadePrivateCartridgeV1(
            gameID: gameID,
            gameVersion: gameVersion,
            fileURL: directoryURL.appendingPathComponent(leaf, isDirectory: false)
        )
    }

    private func rejectUnsafeExistingFile(_ url: URL) throws {
        if (try? FileManager.default.destinationOfSymbolicLink(atPath: url.path)) != nil {
            throw TinyArcadePrivateLibraryError.unsafeStoredFile
        }
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true else {
                throw TinyArcadePrivateLibraryError.unsafeStoredFile
            }
        } catch let error as TinyArcadePrivateLibraryError {
            throw error
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
    }

    private func ensureCapacity(for destination: URL) throws {
        if FileManager.default.fileExists(atPath: destination.path) { return }
        do {
            let count = try FileManager.default.contentsOfDirectory(
                at: directoryURL,
                includingPropertiesForKeys: nil,
                options: [.skipsHiddenFiles]
            ).lazy.filter { $0.pathExtension == "wasm" }.prefix(
                Self.maximumCartridgeCount
            ).count
            guard count < Self.maximumCartridgeCount else {
                throw TinyArcadePrivateLibraryError.tooManyCartridges
            }
        } catch let error as TinyArcadePrivateLibraryError {
            throw error
        } catch {
            throw TinyArcadePrivateLibraryError.storageFailure
        }
    }

    private static func identity(from leaf: String) -> (gameID: String, gameVersion: String)? {
        guard leaf.hasSuffix(".wasm") else { return nil }
        let stem = leaf.dropLast(5)
        let fields = stem.split(separator: "@", omittingEmptySubsequences: false)
        guard fields.count == 2 else { return nil }
        let gameID = String(fields[0])
        let gameVersion = String(fields[1])
        guard validGameID(gameID), validVersion(gameVersion) else { return nil }
        return (gameID, gameVersion)
    }

    private static func validGameID(_ value: String) -> Bool {
        (3...128).contains(value.utf8.count) && value.utf8.allSatisfy {
            (97...122).contains($0) || (48...57).contains($0) || [46, 95, 45].contains($0)
        }
    }

    private static func validVersion(_ value: String) -> Bool {
        (1...64).contains(value.utf8.count) && value.utf8.allSatisfy {
            (65...90).contains($0) || (97...122).contains($0)
                || (48...57).contains($0) || [46, 95, 43, 45].contains($0)
        }
    }
}
