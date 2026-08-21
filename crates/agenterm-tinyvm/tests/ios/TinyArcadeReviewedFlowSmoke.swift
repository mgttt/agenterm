import CryptoKit
import Foundation
import TinyArcade

private final class ReviewedFixtureState: @unchecked Sendable {
    private let lock = NSLock()
    private var activeSlowRequests = 0

    func begin() {
        lock.lock()
        activeSlowRequests += 1
        lock.unlock()
    }

    func end() {
        lock.lock()
        activeSlowRequests -= 1
        lock.unlock()
    }

    func isActive() -> Bool {
        lock.lock()
        let result = activeSlowRequests > 0
        lock.unlock()
        return result
    }
}

private final class ReviewedFixtureURLProtocol: URLProtocol, @unchecked Sendable {
    static let state = ReviewedFixtureState()
    nonisolated(unsafe) static var cartridge = Data()
    nonisolated(unsafe) static var catalog = Data()
    nonisolated(unsafe) static var slowCatalog = Data()

    private let lock = NSLock()
    private var stopped = false
    private var delayedWork: DispatchWorkItem?
    private var slowCounted = false

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == "reviewed.test"
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url else { return }
        switch url.path {
        case "/catalog-v1.json":
            respond(mime: "application/json", body: Self.catalog)
        case "/slow-catalog-v1.json":
            respond(mime: "application/json", body: Self.slowCatalog)
        case "/wasm/paddle-guard-0.1.0.wasm":
            respond(mime: "application/wasm", body: Self.cartridge)
        case "/wasm/slow-paddle-guard-0.1.0.wasm":
            Self.state.begin()
            let work = DispatchWorkItem { [weak self] in
                self?.respond(mime: "application/wasm", body: Self.cartridge)
                self?.finishSlow()
            }
            lock.lock()
            delayedWork = work
            slowCounted = true
            lock.unlock()
            DispatchQueue.global().asyncAfter(deadline: .now() + 1, execute: work)
        default:
            respond(status: 404, mime: "text/plain", body: Data())
        }
    }

    override func stopLoading() {
        lock.lock()
        stopped = true
        let work = delayedWork
        delayedWork = nil
        let shouldEnd = slowCounted
        slowCounted = false
        lock.unlock()
        if shouldEnd { Self.state.end() }
        work?.cancel()
    }

    private func finishSlow() {
        lock.lock()
        let shouldEnd = slowCounted
        slowCounted = false
        lock.unlock()
        if shouldEnd { Self.state.end() }
    }

    private func respond(status: Int = 200, mime: String, body: Data) {
        lock.lock()
        let stopped = self.stopped
        lock.unlock()
        guard !stopped, let url = request.url else { return }
        let response = HTTPURLResponse(
            url: url,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: [
                "Content-Type": mime,
                "Content-Length": String(body.count),
            ]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if !body.isEmpty { client?.urlProtocol(self, didLoad: body) }
        client?.urlProtocolDidFinishLoading(self)
    }
}

@main
private struct TinyArcadeReviewedFlowSmoke {
    static let gameID = "com.partnernet.paddle-guard"
    static let gameVersion = "0.1.0"
    static let keyID = "reviewed-flow-test"

    @MainActor
    static func main() async throws {
        guard CommandLine.arguments.count == 2 else {
            preconditionFailure("reviewed flow smoke requires Paddle Guard .wasm")
        }
        let cartridge = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let privateKey = try Curve25519.Signing.PrivateKey(
            rawRepresentation: Data(repeating: 0x5a, count: 32)
        )
        let entry = try signedEntry(cartridge: cartridge, privateKey: privateKey)
        ReviewedFixtureURLProtocol.cartridge = cartridge
        ReviewedFixtureURLProtocol.catalog = catalog(entry: entry, cartridge: "paddle-guard-0.1.0.wasm")
        ReviewedFixtureURLProtocol.slowCatalog = catalog(
            entry: entry,
            cartridge: "slow-paddle-guard-0.1.0.wasm"
        )

        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [ReviewedFixtureURLProtocol.self]
        let transport = TinyArcadeHTTPSClientV1(
            configuration: configuration,
            timeoutInterval: 5
        )
        let cacheURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-reviewed-flow-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: cacheURL) }
        let cache = try TinyArcadeCartridgeCacheV1(
            directoryURL: cacheURL,
            maxWasmBytes: 64 * 1_024
        )
        let trust = try TinyArcadeTrustStoreV1()
        try trust.addKey(id: keyID, ed25519PublicKey: privateKey.publicKey.rawRepresentation)
        do {
            _ = try TinyArcadeReviewedLibraryV1(
                transport: transport,
                cache: cache,
                trustStore: trust
            )
            preconditionFailure("App Store baseline must disable reviewed library")
        } catch let error as TinyArcadeDistributionPolicyError {
            precondition(error == .externalCartridgesDisabled)
        }
        let library = try TinyArcadeReviewedLibraryV1(
            transport: transport,
            cache: cache,
            trustStore: trust,
            distributionPolicy: .sdkTestExternalCartridges
        )
        let root = URL(string: "https://reviewed.test/")!
        let base = root.appendingPathComponent("wasm", isDirectory: true)

        let slowCatalog = try await library.fetchCatalog(
            at: root.appendingPathComponent("slow-catalog-v1.json"),
            cartridgeBaseURL: base
        )
        let cancelled = Task { @MainActor in
            try await library.installAndOpen(slowCatalog.games[0])
        }
        for _ in 0..<10_000 where !ReviewedFixtureURLProtocol.state.isActive() {
            await Task.yield()
        }
        precondition(ReviewedFixtureURLProtocol.state.isActive())
        do {
            _ = try await library.installAndOpen(slowCatalog.games[0])
            preconditionFailure("parallel reviewed install must fail")
        } catch let error as TinyArcadeReviewedLibraryError {
            precondition(error == .operationInProgress)
        }
        cancelled.cancel()
        do {
            _ = try await cancelled.value
            preconditionFailure("cancelled reviewed install must fail")
        } catch {}
        do {
            _ = try cache.loadActive(entry: entry, trustStore: trust)
            preconditionFailure("cancelled download must not activate cache")
        } catch {}

        let currentCatalog = try await library.fetchCatalog(
            at: root.appendingPathComponent("catalog-v1.json"),
            cartridgeBaseURL: base
        )
        do {
            _ = try await library.installAndOpen(currentCatalog.games[0]) { config in
                config.max_memory_pages = 1
            }
            preconditionFailure("runtime preflight with an impossible memory limit must fail")
        } catch {}
        do {
            _ = try cache.loadActive(entry: entry, trustStore: trust)
            preconditionFailure("failed runtime preflight must not activate cache")
        } catch {}

        let runtime = try await library.installAndOpen(currentCatalog.games[0]) { config in
            config.max_memory_pages = 17
            config.max_steps = 500_000
            config.max_render_bytes = 20 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 128
        }
        let origin = try runtime.origin()
        precondition(origin == .officialReviewed)
        let frame = try runtime.tickMedia(buttons: 1 << 4, clockMilliseconds: 0)
        guard case let .indexed2D(indexed) = frame.renderFrame else {
            preconditionFailure("reviewed Paddle Guard must emit indexed2d")
        }
        precondition(indexed.width == 160 && indexed.height == 120)
        try runtime.close()

        var tampered = cartridge
        tampered[tampered.startIndex] ^= 0xff
        ReviewedFixtureURLProtocol.cartridge = tampered
        do {
            _ = try await library.installAndOpen(currentCatalog.games[0])
            preconditionFailure("tampered reviewed bytes must fail before activation")
        } catch {}
        ReviewedFixtureURLProtocol.cartridge = cartridge

        let reopened = try library.openActive(currentCatalog.games[0]) { config in
            config.max_memory_pages = 17
            config.max_steps = 500_000
            config.max_render_bytes = 20 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 128
        }
        let reopenedOrigin = try reopened.origin()
        precondition(reopenedOrigin == .officialReviewed)
        _ = try reopened.tickMedia(buttons: 0, clockMilliseconds: 16)
        try reopened.close()
        try trust.revokeContent(sha256: entry.wasmSHA256)
        do {
            _ = try library.openActive(currentCatalog.games[0])
            preconditionFailure("live revocation must reject cached reviewed bytes")
        } catch {}
        print("OK: reviewed HTTPS → preflight → atomic activation → cached reopen")
    }

    static func signedEntry(
        cartridge: Data,
        privateKey: Curve25519.Signing.PrivateKey
    ) throws -> TinyArcadeReviewedCatalogEntry {
        let digest = Data(SHA256.hash(data: cartridge))
        let message = signingBytes(wasmLength: UInt64(cartridge.count), sha256: digest)
        return TinyArcadeReviewedCatalogEntry(
            gameID: gameID,
            gameVersion: gameVersion,
            abiVersion: 1,
            stateVersion: 1,
            wasmLength: UInt64(cartridge.count),
            wasmSHA256: digest,
            signingKeyID: keyID,
            signature: try privateKey.signature(for: message)
        )
    }

    static func signingBytes(wasmLength: UInt64, sha256: Data) -> Data {
        var message = Data("TinyArcade signed catalog entry v1\0".utf8)
        append(UInt32(1), to: &message)
        append(gameID, to: &message)
        append(gameVersion, to: &message)
        append(UInt32(1), to: &message)
        append(UInt32(1), to: &message)
        append(wasmLength, to: &message)
        message.append(sha256)
        append(keyID, to: &message)
        return message
    }

    static func append<T: FixedWidthInteger>(_ value: T, to data: inout Data) {
        withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) }
    }

    static func append(_ value: String, to data: inout Data) {
        append(UInt16(value.utf8.count), to: &data)
        data.append(contentsOf: value.utf8)
    }

    static func catalog(entry: TinyArcadeReviewedCatalogEntry, cartridge: String) -> Data {
        Data(
            """
            {
              "schema_version": 1,
              "catalog_id": "com.partnernet.reviewed-test",
              "games": [{
                "game_id": "\(entry.gameID)",
                "game_version": "\(entry.gameVersion)",
                "title": "Paddle Guard",
                "summary": "Reviewed flow fixture.",
                "cartridge": "\(cartridge)",
                "abi_version": \(entry.abiVersion),
                "state_version": \(entry.stateVersion),
                "wasm_length": \(entry.wasmLength),
                "wasm_sha256": "\(entry.wasmSHA256.map { String(format: "%02x", $0) }.joined())",
                "signing_key_id": "\(entry.signingKeyID)",
                "signature": "\(entry.signature.base64EncodedString())"
              }]
            }
            """.utf8
        )
    }
}
