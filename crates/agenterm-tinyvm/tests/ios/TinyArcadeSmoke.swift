import AVFoundation
import Foundation
import UIKit
import TinyArcade

private final class TinyArcadeFixtureConcurrency: @unchecked Sendable {
    private let lock = NSLock()
    private var active = 0
    private var peak = 0

    func reset() {
        lock.lock()
        active = 0
        peak = 0
        lock.unlock()
    }

    func begin() {
        lock.lock()
        active += 1
        peak = max(peak, active)
        lock.unlock()
    }

    func end() {
        lock.lock()
        active -= 1
        lock.unlock()
    }

    func peakValue() -> Int {
        lock.lock()
        let value = peak
        lock.unlock()
        return value
    }

    func activeValue() -> Int {
        lock.lock()
        let value = active
        lock.unlock()
        return value
    }
}

private final class TinyArcadeFixtureURLProtocol: URLProtocol, @unchecked Sendable {
    static let concurrency = TinyArcadeFixtureConcurrency()
    private let lock = NSLock()
    private var stopped = false
    private var delayedWork: DispatchWorkItem?

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == "tinyarcade.test"
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        guard let url = request.url else { return }
        switch url.path {
        case "/catalog-v1.json":
            respond(status: 200, mime: "application/json", body: Self.catalogData())
        case "/short-catalog.json":
            respond(
                status: 200,
                mime: "application/json",
                body: Data(
                    String(decoding: Self.catalogData(), as: UTF8.self)
                        .replacingOccurrences(
                            of: "paddle-guard-0.1.0.wasm",
                            with: "paddle-guard-short-0.1.0.wasm"
                        ).utf8
                )
            )
        case "/wasm/paddle-guard-0.1.0.wasm":
            respond(status: 200, mime: "application/wasm", body: Data(repeating: 0x2a, count: 5_280))
        case "/wasm/paddle-guard-short-0.1.0.wasm":
            respond(status: 200, mime: "application/wasm", body: Data(repeating: 0x2a, count: 5_279))
        case "/oversize.json":
            respond(
                status: 200,
                mime: "application/json",
                body: Data(),
                declaredLength: TinyArcadeCatalogV1.maximumDocumentBytes + 1
            )
        case "/wrong-mime.json":
            respond(status: 200, mime: "text/plain", body: Self.catalogData())
        case "/chunk-oversize.json":
            respond(
                status: 200,
                mime: "application/json",
                body: Data(count: TinyArcadeCatalogV1.maximumDocumentBytes + 1),
                includesLength: false
            )
        case "/redirect.json":
            let response = HTTPURLResponse(
                url: url,
                statusCode: 302,
                httpVersion: "HTTP/1.1",
                headerFields: ["Location": "https://tinyarcade.test/catalog-v1.json"]
            )!
            client?.urlProtocol(
                self,
                wasRedirectedTo: URLRequest(
                    url: URL(string: "https://tinyarcade.test/catalog-v1.json")!
                ),
                redirectResponse: response
            )
        case "/slow.json":
            let work = DispatchWorkItem { [weak self] in
                self?.respond(status: 200, mime: "application/json", body: Self.catalogData())
            }
            lock.lock()
            delayedWork = work
            let stopped = self.stopped
            lock.unlock()
            if stopped {
                work.cancel()
            } else {
                DispatchQueue.global().asyncAfter(deadline: .now() + 1, execute: work)
            }
        case let path where path.hasPrefix("/limited-"):
            Self.concurrency.begin()
            let work = DispatchWorkItem { [weak self] in
                self?.respond(status: 200, mime: "application/json", body: Self.catalogData())
                Self.concurrency.end()
            }
            lock.lock()
            delayedWork = work
            lock.unlock()
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.1, execute: work)
        default:
            respond(status: 404, mime: "text/plain", body: Data())
        }
    }

    override func stopLoading() {
        lock.lock()
        stopped = true
        let work = delayedWork
        delayedWork = nil
        lock.unlock()
        work?.cancel()
    }

    private func respond(
        status: Int,
        mime: String,
        body: Data,
        declaredLength: Int? = nil,
        includesLength: Bool = true
    ) {
        lock.lock()
        let stopped = self.stopped
        lock.unlock()
        guard !stopped, let url = request.url else { return }
        var headers = ["Content-Type": mime]
        if includesLength {
            headers["Content-Length"] = String(declaredLength ?? body.count)
        }
        let response = HTTPURLResponse(
            url: url,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: headers
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if !body.isEmpty { client?.urlProtocol(self, didLoad: body) }
        client?.urlProtocolDidFinishLoading(self)
    }

    static func catalogData() -> Data {
        let signature = Data(repeating: 0, count: 64).base64EncodedString()
        return Data(
            """
            {
              "schema_version": 1,
              "catalog_id": "com.partnernet.tinyarcade",
              "games": [{
                "game_id": "com.partnernet.paddle-guard",
                "game_version": "0.1.0",
                "title": "Paddle Guard",
                "summary": "Defend the field.",
                "localizations": {
                  "zh-Hans": {"title": "护盾弹球", "summary": "守住球场。"}
                },
                "cartridge": "paddle-guard-0.1.0.wasm",
                "abi_version": 1,
                "state_version": 1,
                "wasm_length": 5280,
                "wasm_sha256": "\(String(repeating: "0", count: 64))",
                "signing_key_id": "catalog-test",
                "signature": "\(signature)"
              }]
            }
            """.utf8
        )
    }
}

@main
struct TinyArcadeSmoke {
    static func appendLEB(_ value: Int, to output: inout [UInt8]) {
        var remaining = value
        repeat {
            var byte = UInt8(remaining & 0x7f)
            remaining >>= 7
            if remaining != 0 { byte |= 0x80 }
            output.append(byte)
        } while remaining != 0
    }

    static func appendSignedLEB(_ value: Int, to output: inout [UInt8]) {
        precondition(value >= 0)
        var remaining = value
        while true {
            var byte = UInt8(remaining & 0x7f)
            remaining >>= 7
            let done = remaining == 0 && byte & 0x40 == 0
            if !done { byte |= 0x80 }
            output.append(byte)
            if done { return }
        }
    }

    static func appendName(_ value: String, to output: inout [UInt8]) {
        let bytes = Array(value.utf8)
        appendLEB(bytes.count, to: &output)
        output.append(contentsOf: bytes)
    }

    static func appendSection(_ id: UInt8, _ payload: [UInt8], to module: inout [UInt8]) {
        module.append(id)
        appendLEB(payload.count, to: &module)
        module.append(contentsOf: payload)
    }

    static func functionBody(_ code: [UInt8]) -> [UInt8] {
        [0] + code
    }

    static func nativeCartridge(renderLength: Int = 26) -> Data {
        var module: [UInt8] = [0, 97, 115, 109, 1, 0, 0, 0]
        let capability = "fan:physics/v1"
        var manifest: [UInt8] = []
        appendName("tinyarcade.manifest.v1", to: &manifest)
        manifest += Array("TAM1".utf8) + [1, 0, 0, 0, 1, 0, 0, 0]
        for value in ["c.native", "1.0.0"] {
            let bytes = Array(value.utf8)
            manifest += [UInt8(bytes.count), 0] + bytes
        }
        manifest += [1, 0, UInt8(capability.utf8.count), 0] + Array(capability.utf8)
        appendSection(0, manifest, to: &module)
        appendSection(1, [2, 0x60, 0, 1, 0x7f, 0x60, 2, 0x7f, 0x7f, 1, 0x7f], to: &module)
        var imports: [UInt8] = [3]
        for (namespace, field, typeIndex) in [
            (capability, "step_world", UInt8(1)),
            ("tinyarcade:core/v1", "indexed2d_version", UInt8(0)),
            ("tinyarcade:core/v1", "submit_render", UInt8(1)),
        ] {
            appendName(namespace, to: &imports)
            appendName(field, to: &imports)
            imports += [0, typeIndex]
        }
        appendSection(2, imports, to: &module)
        appendSection(3, [5, 0, 0, 0, 0, 0], to: &module)
        appendSection(5, [1, 0, 1], to: &module)
        var exports: [UInt8] = [5]
        for (field, index) in [
            ("game_abi_version", 3),
            ("game_init", 4),
            ("game_tick", 5),
            ("game_suspend", 6),
            ("game_resume", 7),
        ] {
            appendName(field, to: &exports)
            exports.append(0)
            appendLEB(index, to: &exports)
        }
        appendSection(7, exports, to: &module)
        var tick: [UInt8] = [
            0x10, 1, 0x1a,
            0x41, 0x28, 0x41, 2, 0x10, 0, 0x1a,
            0x41, 0x28, 0x41, 2, 0x10, 0, 0x1a,
            0x41, 0,
            0x41,
        ]
        appendSignedLEB(renderLength, to: &tick)
        tick += [0x10, 2, 0x1a, 0x41, 0, 0x0b]
        let functions = [
            functionBody([0x41, 1, 0x0b]),
            functionBody([0x41, 0, 0x0b]),
            functionBody(tick),
            functionBody([0x41, 0, 0x0b]),
            functionBody([0x41, 0, 0x0b]),
        ]
        var code: [UInt8] = [5]
        for function in functions {
            appendLEB(function.count, to: &code)
            code += function
        }
        appendSection(10, code, to: &module)
        return Data(module)
    }

    static func indexedFrame(lastPixel: UInt8) -> [UInt8] {
        [
            84, 65, 73, 50, 1, 0, 16, 0,
            2, 0, 1, 0, 2, 0, 0, 0,
            255, 0, 0, 255, 0, 255, 0, 128,
            0, lastPixel,
        ]
    }

    static func classicIndexedFrame() -> [UInt8] {
        let width = 320
        let height = 200
        var bytes: [UInt8] = [
            84, 65, 73, 50, 1, 0, 16, 0,
            UInt8(width & 0xff), UInt8(width >> 8), UInt8(height), 0,
            0, 1, 0, 0,
        ]
        bytes.reserveCapacity(16 + 256 * 4 + width * height)
        for color in 0..<256 {
            bytes += [UInt8(color), UInt8(255 - color), UInt8(color ^ 0x55), 255]
        }
        for pixel in 0..<(width * height) {
            bytes.append(UInt8(pixel & 0xff))
        }
        return bytes
    }

    @MainActor
    static func main() async throws {
        precondition(tinyarcade_v1_abi_version() == TINYARCADE_ABI_VERSION)
        var config = tinyarcade_config_v1()
        precondition(tinyarcade_v1_default_config(&config) == TINYARCADE_OK)
        precondition(config.struct_size == MemoryLayout<tinyarcade_config_v1>.size)
        _ = TinyArcadeRuntimeV1.self

        do {
            try TinyArcadeRuntimeV1.requireStableCopyLength(
                1,
                expected: 2,
                context: "smoke output"
            )
            preconditionFailure("changed two-pass copy length must fail")
        } catch let error as TinyArcadeRuntimeError {
            precondition(error.status == Int32(TINYARCADE_DECODE_ERROR.rawValue))
            precondition(error.message == "smoke output length changed during copy")
        }

        let nativeBytes = nativeCartridge()
        let descriptor = try TinyArcadeCartridgeDescriptorV1.inspect(nativeBytes)
        precondition(descriptor.gameID == "c.native")
        precondition(descriptor.gameVersion == "1.0.0")
        precondition(descriptor.abiVersion == 1 && descriptor.stateVersion == 1)
        precondition(descriptor.wasmLength == UInt32(nativeBytes.count))
        precondition(descriptor.nativeCapabilities == ["fan:physics/v1"])
        precondition(!descriptor.isCoreOnly)
        precondition(descriptor.functionImports.count == 3)
        precondition(
            descriptor.functionImports.first == TinyArcadeFunctionImportV1(
                module: "fan:physics/v1",
                field: "step_world",
                parameterCount: 2,
                resultCount: 1,
                importClass: .native
            )
        )
        var profileHandlerCalls = 0
        let hostProfile = try TinyArcadeHostProfileV1.appBuild(
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { _, _ in
                    profileHandlerCalls += 1
                    return [0]
                },
            ]
        )
        precondition(hostProfile.encoded.prefix(4) == Data("TAH1".utf8))
        let profileDescriptor = try hostProfile.inspectCompatibleCartridge(nativeBytes)
        precondition(profileDescriptor == descriptor)
        precondition(profileHandlerCalls == 0, "static profile check must not call app code")
        let coreOnlyProfile = try TinyArcadeHostProfileV1.appBuild()
        do {
            _ = try coreOnlyProfile.inspectCompatibleCartridge(nativeBytes)
            preconditionFailure("core-only profile must reject a native import")
        } catch let error as TinyArcadeRuntimeError {
            precondition(error.status == Int32(TINYARCADE_GUEST_TRAP.rawValue))
        }
        print("OK: exact iOS app host profile → converter-safe TAH1 → static import check")
        let privateDescriptorDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-private-descriptor-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: privateDescriptorDirectory) }
        precondition(
            TinyArcadeDistributionPolicyV1.appStoreBundledOnly.externalApprovalReference == nil
        )
        do {
            _ = try TinyArcadeDistributionPolicyV1.appleApprovedExternalCartridges(
                approvalReference: "bad ref"
            )
            preconditionFailure("invalid Apple approval reference must fail")
        } catch let error as TinyArcadeDistributionPolicyError {
            precondition(error == .invalidAppleApprovalReference)
        }
        let recordedApproval = try TinyArcadeDistributionPolicyV1
            .appleApprovedExternalCartridges(approvalReference: "app-review-case-123")
        precondition(recordedApproval.externalApprovalReference == "app-review-case-123")
        do {
            _ = try TinyArcadePrivateLibraryV1(directoryURL: privateDescriptorDirectory)
            preconditionFailure("App Store baseline must disable private libraries")
        } catch let error as TinyArcadeDistributionPolicyError {
            precondition(error == .externalCartridgesDisabled)
        }
        precondition(
            !FileManager.default.fileExists(atPath: privateDescriptorDirectory.path),
            "bundled-only refusal must precede private directory creation"
        )
        let privateDescriptorLibrary = try TinyArcadePrivateLibraryV1(
            directoryURL: privateDescriptorDirectory,
            distributionPolicy: .sdkTestExternalCartridges
        )
        do {
            _ = try privateDescriptorLibrary.importCartridge(nativeBytes)
            preconditionFailure("private import must report unavailable native capabilities")
        } catch let error as TinyArcadePrivateLibraryError {
            precondition(
                error == .unsupportedNativeCapabilities(["fan:physics/v1"])
            )
        }
        print("OK: App Store bundled-only default → explicit approval record → external SDK test policy")

        let cacheDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("tinyarcade-ios-cache-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: cacheDirectory) }
        let cache = try TinyArcadeCartridgeCacheV1(
            directoryURL: cacheDirectory,
            maxWasmBytes: 64 * 1_024
        )
        let emptyTrust = try TinyArcadeTrustStoreV1()
        let untrustedEntry = TinyArcadeReviewedCatalogEntry(
            gameID: "c.native",
            gameVersion: "1.0.0",
            abiVersion: 1,
            stateVersion: 1,
            wasmLength: UInt64(nativeCartridge().count),
            wasmSHA256: Data(repeating: 0, count: 32),
            signingKeyID: "missing-test-key",
            signature: Data(repeating: 0, count: 64)
        )
        do {
            try cache.activate(
                entry: untrustedEntry,
                cartridge: nativeCartridge(),
                trustStore: emptyTrust
            )
            preconditionFailure("untrusted cartridge must not enter the iOS cache")
        } catch let error as TinyArcadeRuntimeError {
            precondition(error.status == Int32(TINYARCADE_TRUST_ERROR.rawValue))
        }
        precondition(FileManager.default.fileExists(atPath: cacheDirectory.path))
        precondition(
            !FileManager.default.fileExists(
                atPath: cacheDirectory.appendingPathComponent("active/c.native.state").path
            )
        )
        try cache.close()
        try cache.close()
        try emptyTrust.close()

        let catalogData = TinyArcadeFixtureURLProtocol.catalogData()
        let catalogBaseURL = URL(string: "https://partnernetsoftware.com/wasm/")!
        let catalog = try TinyArcadeCatalogV1.decode(
            catalogData,
            cartridgeBaseURL: catalogBaseURL
        )
        precondition(catalog.games.count == 1)
        precondition(catalog.hostProfile == nil)
        let catalogGame = catalog.games[0]
        precondition(catalogGame.cartridgeURL.absoluteString == "https://partnernetsoftware.com/wasm/paddle-guard-0.1.0.wasm")
        precondition(catalogGame.localized(for: "zh-Hans").title == "护盾弹球")
        precondition(catalogGame.localized(for: "zh-Hans-CN").title == "护盾弹球")
        let deepLink = catalogGame.deepLinkURL()!
        precondition(deepLink.absoluteString == "tinyarcade://game/com.partnernet.paddle-guard")
        precondition(catalog.game(forDeepLink: deepLink)?.entry.gameID == catalogGame.entry.gameID)
        precondition(
            catalog.game(
                forDeepLink: URL(string: deepLink.absoluteString + "?run=1")!
            ) == nil
        )
        let traversalCatalog = Data(
            String(decoding: catalogData, as: UTF8.self)
                .replacingOccurrences(
                    of: "paddle-guard-0.1.0.wasm",
                    with: "../paddle-guard-0.1.0.wasm"
                ).utf8
        )
        do {
            _ = try TinyArcadeCatalogV1.decode(
                traversalCatalog,
                cartridgeBaseURL: catalogBaseURL
            )
            preconditionFailure("catalog traversal must fail")
        } catch let error as TinyArcadeCatalogDecodeError {
            precondition(error == .invalidEntry(0))
        }
        let nonASCIIHashCatalog = Data(
            String(decoding: catalogData, as: UTF8.self)
                .replacingOccurrences(
                    of: String(repeating: "0", count: 64),
                    with: String(repeating: "é", count: 32)
                ).utf8
        )
        do {
            _ = try TinyArcadeCatalogV1.decode(
                nonASCIIHashCatalog,
                cartridgeBaseURL: catalogBaseURL
            )
            preconditionFailure("non-ASCII digest must fail without trapping")
        } catch let error as TinyArcadeCatalogDecodeError {
            precondition(error == .invalidEntry(0))
        }

        let transportConfiguration = URLSessionConfiguration.ephemeral
        transportConfiguration.protocolClasses = [TinyArcadeFixtureURLProtocol.self]
        let transport = TinyArcadeHTTPSClientV1(
            configuration: transportConfiguration,
            timeoutInterval: 5
        )
        let fixtureRoot = URL(string: "https://tinyarcade.test/")!
        do {
            _ = try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("catalog-v1.json"),
                cartridgeBaseURL: URL(string: "https://other.test/wasm/")!
            )
            preconditionFailure("catalog and cartridge origins must match")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .invalidURL)
        }
        let fixtureCatalog = try await transport.fetchCatalog(
            at: fixtureRoot.appendingPathComponent("catalog-v1.json"),
            cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
        )
        precondition(fixtureCatalog.games.count == 1)
        let transportedCartridge = try await transport.fetchCartridge(fixtureCatalog.games[0])
        precondition(transportedCartridge == Data(repeating: 0x2a, count: 5_280))

        let shortCatalog = try await transport.fetchCatalog(
            at: fixtureRoot.appendingPathComponent("short-catalog.json"),
            cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
        )
        do {
            _ = try await transport.fetchCartridge(shortCatalog.games[0])
            preconditionFailure("cartridge length must match its signed entry")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .lengthMismatch)
        }

        do {
            _ = try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("oversize.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
            preconditionFailure("declared oversize catalog must fail before body buffering")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .responseTooLarge)
        }
        do {
            _ = try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("chunk-oversize.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
            preconditionFailure("undeclared oversize body must fail while streaming")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .responseTooLarge)
        }
        do {
            _ = try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("wrong-mime.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
            preconditionFailure("wrong catalog MIME must fail")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .unsupportedContentType)
        }
        do {
            _ = try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("redirect.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
            preconditionFailure("catalog redirects must fail")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .redirectRejected)
        }
        let cancelledRequest = Task {
            try await transport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("slow.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
        }
        cancelledRequest.cancel()
        do {
            _ = try await cancelledRequest.value
            preconditionFailure("cancelled catalog request must fail")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .cancelled)
        }
        TinyArcadeFixtureURLProtocol.concurrency.reset()
        let limitedTransport = TinyArcadeHTTPSClientV1(
            configuration: transportConfiguration,
            timeoutInterval: 5,
            maximumConcurrentRequests: 2
        )
        try await withThrowingTaskGroup(of: Void.self) { group in
            for index in 0..<6 {
                group.addTask {
                    _ = try await limitedTransport.fetchCatalog(
                        at: fixtureRoot.appendingPathComponent("limited-\(index).json"),
                        cartridgeBaseURL: fixtureRoot.appendingPathComponent(
                            "wasm",
                            isDirectory: true
                        )
                    )
                }
            }
            try await group.waitForAll()
        }
        precondition(TinyArcadeFixtureURLProtocol.concurrency.peakValue() == 2)

        TinyArcadeFixtureURLProtocol.concurrency.reset()
        let noQueueTransport = TinyArcadeHTTPSClientV1(
            configuration: transportConfiguration,
            timeoutInterval: 5,
            maximumConcurrentRequests: 1,
            maximumQueuedRequests: 0
        )
        let occupyingRequest = Task {
            try await noQueueTransport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("limited-occupying.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
        }
        for _ in 0..<10_000 where TinyArcadeFixtureURLProtocol.concurrency.activeValue() == 0 {
            await Task.yield()
        }
        precondition(TinyArcadeFixtureURLProtocol.concurrency.activeValue() == 1)
        do {
            _ = try await noQueueTransport.fetchCatalog(
                at: fixtureRoot.appendingPathComponent("limited-rejected.json"),
                cartridgeBaseURL: fixtureRoot.appendingPathComponent("wasm", isDirectory: true)
            )
            preconditionFailure("a saturated zero-queue transport must reject")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .requestQueueFull)
        }
        _ = try await occupyingRequest.value

        var nativeCalls = 0
        let nativeRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { parameters, memory in
                    precondition(parameters == [40, 2])
                    let indexedFrame = Self.indexedFrame(lastPixel: 1)
                    precondition(memory.count >= indexedFrame.count)
                    for (index, value) in indexedFrame.enumerated() { memory[index] = value }
                    nativeCalls += 1
                    return [42]
                },
            ]
        )
        let media = try nativeRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
        precondition(nativeCalls == 2)
        guard case let .indexed2D(indexedFrame) = media.renderFrame else {
            preconditionFailure("native smoke should decode indexed2d")
        }
        precondition(indexedFrame.width == 2 && indexedFrame.height == 1)
        precondition(indexedFrame.paletteRGBA == [0xff00_00ff, 0x8000_ff00])
        precondition(indexedFrame.pixels == Data([0, 1]))
        media.render.withUnsafeBytes { renderBytes in
            indexedFrame.withPixelBytes { pixelBytes in
                precondition(
                    pixelBytes.baseAddress == renderBytes.baseAddress?.advanced(by: 24),
                    "validated pixel planes must share the one Swift-owned render copy"
                )
            }
        }
        let expectedRGBA = Data([255, 0, 0, 255, 0, 255, 0, 128])
        precondition(indexedFrame.rgba8888() == expectedRGBA)
        let image = try indexedFrame.makeCGImage()
        precondition(image.width == 2 && image.height == 1)
        precondition(image.bitsPerPixel == 32 && image.bytesPerRow == 8)
        precondition(image.shouldInterpolate == false)
        precondition(image.alphaInfo == .last)
        precondition(image.bitmapInfo.contains(.byteOrder32Big))
        precondition(image.colorSpace?.name == CGColorSpace.sRGB)
        guard let providerData = image.dataProvider?.data else {
            preconditionFailure("indexed image must retain its pixel provider")
        }
        precondition(providerData as Data == expectedRGBA)
        let view = TinyArcadeIndexed2DView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        try view.display(indexedFrame)
        precondition(view.layer.contents != nil)
        precondition(view.layer.contentsGravity == .resizeAspect)
        precondition(view.layer.magnificationFilter == .nearest)
        precondition(view.layer.minificationFilter == .nearest)
        view.clear()
        precondition(view.layer.contents == nil)
        try nativeRuntime.close()

        let malformedRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { _, memory in
                    let malformed = Self.indexedFrame(lastPixel: 2)
                    for (index, value) in malformed.enumerated() { memory[index] = value }
                    return [42]
                },
            ]
        )
        do {
            _ = try malformedRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
            preconditionFailure("out-of-palette indexed pixel must fail")
        } catch let error as TinyArcadeRuntimeError {
            precondition(error.status == Int32(TINYARCADE_DECODE_ERROR.rawValue))
        }
        try malformedRuntime.close()

        let classicBytes = Self.classicIndexedFrame()
        precondition(classicBytes.count == 65_040)
        let classicRuntime = try TinyArcadeRuntimeV1(
            cartridge: nativeCartridge(renderLength: classicBytes.count),
            nativeFunctions: [
                TinyArcadeNativeFunctionV1(
                    module: "fan:physics/v1",
                    field: "step_world",
                    parameterCount: 2,
                    resultCount: 1,
                    maxCallsPerLifecycle: 2
                ) { _, memory in
                    for (index, value) in classicBytes.enumerated() { memory[index] = value }
                    return [42]
                },
            ]
        )
        let classicMedia = try classicRuntime.tickMedia(buttons: 0, clockMilliseconds: 0)
        guard case let .indexed2D(classicFrame) = classicMedia.renderFrame else {
            preconditionFailure("classic smoke should decode indexed2d")
        }
        precondition(classicFrame.width == 320 && classicFrame.height == 200)
        precondition(classicFrame.paletteRGBA.count == 256)
        precondition(classicFrame.pixels.count == 64_000)
        let classicView = TinyArcadeIndexed2DView(
            frame: CGRect(x: 0, y: 0, width: 390, height: 844)
        )
        let renderIterations = 120
        let renderStart = ProcessInfo.processInfo.systemUptime
        for _ in 0..<renderIterations { try classicView.display(classicFrame) }
        let renderAverageMilliseconds = (
            ProcessInfo.processInfo.systemUptime - renderStart
        ) * 1_000 / Double(renderIterations)
        precondition(renderAverageMilliseconds < 16.0)
        print(
            String(
                format: "OK: indexed2d 320x200 native presentation avg=%.3fms",
                renderAverageMilliseconds
            )
        )
        try classicRuntime.close()

        guard CommandLine.arguments.count >= 2 else { return }
        let cartridge = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        do {
            _ = try TinyArcadeRuntimeV1(privateCartridge: cartridge)
            preconditionFailure("App Store baseline must disable external runtime open")
        } catch let error as TinyArcadeDistributionPolicyError {
            precondition(error == .externalCartridgesDisabled)
        }
        let runtime = try TinyArcadeRuntimeV1(
            privateCartridge: cartridge,
            distributionPolicy: .sdkTestExternalCartridges
        ) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        let origin = try runtime.origin()
        precondition(origin == .privateUser)
        let frame = try runtime.tick(buttons: 0, clockMilliseconds: 0)
        precondition(frame.grid3D.width == 5)
        precondition(frame.grid3D.depth == 5)
        precondition(frame.grid3D.height == 10)
        precondition(frame.grid3D.cells.count == 8)
        precondition(frame.tones.isEmpty)
        let snapshot = try runtime.suspend()
        let restored = try TinyArcadeRuntimeV1(
            privateCartridge: cartridge,
            distributionPolicy: .sdkTestExternalCartridges
        ) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        try restored.resume(snapshot: snapshot)
        let dropped = try restored.tick(buttons: 1 << 7, clockMilliseconds: 1)
        precondition(dropped.grid3D.score >= 10)
        precondition(dropped.tones.count == 1)

        let measured = try TinyArcadeRuntimeV1(
            privateCartridge: cartridge,
            distributionPolicy: .sdkTestExternalCartridges
        ) { config in
            config.max_memory_pages = 17
            config.max_steps = 100_000
            config.max_render_bytes = 4 * 1_024
            config.max_audio_bytes = 64
            config.max_state_bytes = 512
        }
        var milliseconds: [Double] = []
        milliseconds.reserveCapacity(600)
        var depthMaxSteps: UInt64 = 0
        var depthMaxPages: UInt32 = 0
        for index in 0..<600 {
            let started = ProcessInfo.processInfo.systemUptime
            let output = try measured.tick(buttons: 0, clockMilliseconds: UInt32(index * 16))
            milliseconds.append((ProcessInfo.processInfo.systemUptime - started) * 1_000)
            let stats = try measured.lastExecutionStats()
            let extendedStats = try measured.lastExecutionStatsV2()
            precondition(stats.lifecycle == .tick)
            precondition(stats.renderBytes == UInt32(output.render.count))
            precondition(stats.audioBytes == UInt32(output.audio.count))
            precondition(stats.wasmSteps > 0 && stats.wasmSteps <= 100_000)
            precondition(stats.memoryPages <= 17)
            precondition(extendedStats.lifecycle == stats.lifecycle)
            precondition(extendedStats.wasmSteps == stats.wasmSteps)
            precondition(extendedStats.peakCallDepth > 0 && extendedStats.peakCallDepth <= 512)
            precondition(extendedStats.peakActivationSlots > 0 && extendedStats.peakActivationSlots <= 1_048_576)
            depthMaxSteps = max(depthMaxSteps, stats.wasmSteps)
            depthMaxPages = max(depthMaxPages, stats.memoryPages)
        }
        milliseconds.sort()
        let average = milliseconds.reduce(0, +) / Double(milliseconds.count)
        let p95 = milliseconds[Int(Double(milliseconds.count - 1) * 0.95)]
        let maximum = milliseconds.last ?? 0
        precondition(p95 < 8, "Depth Well simulator p95 exceeded 8 ms")
        print(
            "OK: Depth Well in iOS Simulator; "
                + String(
                    format: "600 frames avg=%.3fms p95=%.3fms max=%.3fms fuel=%llu pages=%u",
                    average, p95, maximum, depthMaxSteps, depthMaxPages
                )
        )
        try runtime.close()
        try restored.close()
        try measured.close()

        guard CommandLine.arguments.count >= 3 else { return }
        let paddleCartridge = try Data(
            contentsOf: URL(fileURLWithPath: CommandLine.arguments[2])
        )
        let makePaddleRuntime: () throws -> TinyArcadeRuntimeV1 = {
            try TinyArcadeRuntimeV1(
                privateCartridge: paddleCartridge,
                distributionPolicy: .sdkTestExternalCartridges
            ) { config in
                config.max_memory_pages = 17
                config.max_steps = 500_000
                config.max_render_bytes = 20 * 1_024
                config.max_audio_bytes = 64
                config.max_state_bytes = 128
            }
        }
        var paddleRuntime = try makePaddleRuntime()
        var paddleFrame = try paddleRuntime.tickMedia(
            buttons: 1 << 4,
            clockMilliseconds: 0
        )
        guard case let .indexed2D(initialPaddle) = paddleFrame.renderFrame else {
            preconditionFailure("Paddle Guard must emit indexed2d")
        }
        precondition(initialPaddle.width == 160 && initialPaddle.height == 120)
        precondition(initialPaddle.paletteRGBA.count == 8)
        precondition(!paddleFrame.tones.isEmpty)
        let paddleWave = TinyArcadeToneSynthesizer.waveData(for: paddleFrame.tones)
        precondition(paddleWave.prefix(4) == Data("RIFF".utf8))
        precondition(paddleWave.subdata(in: 8..<12) == Data("WAVE".utf8))
        precondition(paddleWave.count > 44)
        let declaredPaddlePCMBytes = UInt32(paddleWave[40])
            | UInt32(paddleWave[41]) << 8
            | UInt32(paddleWave[42]) << 16
            | UInt32(paddleWave[43]) << 24
        precondition(Int(declaredPaddlePCMBytes) == paddleWave.count - 44)
        precondition(paddleWave[44...].contains(where: { $0 != 0 }))
        let tonePlayer = TinyArcadeTonePlayer()
        precondition(tonePlayer.waveSynthesisCount == 0)
        try tonePlayer.play(paddleFrame.tones)
        precondition(tonePlayer.waveSynthesisCount == 1)
        precondition(tonePlayer.cachedWaveCount == 1)
        precondition(tonePlayer.cachedWaveBytes == paddleWave.count)
        precondition(tonePlayer.isAudioSessionActive)
        NotificationCenter.default.post(
            name: AVAudioSession.interruptionNotification,
            object: AVAudioSession.sharedInstance(),
            userInfo: [
                AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.began.rawValue
            ]
        )
        precondition(!tonePlayer.isPlaying)
        precondition(!tonePlayer.isAudioSessionActive)
        try tonePlayer.play(paddleFrame.tones)
        precondition(tonePlayer.isPlaying)
        await withCheckedContinuation { continuation in
            DispatchQueue.global().async {
                NotificationCenter.default.post(
                    name: AVAudioSession.routeChangeNotification,
                    object: nil,
                    userInfo: [
                        AVAudioSessionRouteChangeReasonKey:
                            AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue
                    ]
                )
                continuation.resume()
            }
        }
        await Task.yield()
        precondition(!tonePlayer.isPlaying)
        try tonePlayer.play(paddleFrame.tones)
        NotificationCenter.default.post(
            name: AVAudioSession.mediaServicesWereResetNotification,
            object: AVAudioSession.sharedInstance()
        )
        precondition(!tonePlayer.isPlaying)
        precondition(!tonePlayer.isAudioSessionActive)
        try tonePlayer.play(paddleFrame.tones)
        precondition(tonePlayer.isAudioSessionActive)
        precondition(
            tonePlayer.waveSynthesisCount == 1,
            "identical gameplay cues must reuse bounded synthesized WAV bytes"
        )
        try tonePlayer.deactivate()
        for index in 0..<10 {
            _ = tonePlayer.waveDataForPlayback(for: [
                TinyArcadeToneEvent(
                    kind: 1,
                    frequencyHz: UInt16(200 + index),
                    durationMilliseconds: 10,
                    amplitudeMilli: 100
                ),
            ])
        }
        precondition(tonePlayer.cachedWaveCount == TinyArcadeTonePlayer.maximumCachedWaveCount)
        precondition(tonePlayer.cachedWaveBytes <= TinyArcadeTonePlayer.maximumCachedWaveBytes)
        for index in 0..<10 {
            _ = tonePlayer.waveDataForPlayback(for: [
                TinyArcadeToneEvent(
                    kind: 1,
                    frequencyHz: UInt16(400 + index),
                    durationMilliseconds: 2_000,
                    amplitudeMilli: 100
                ),
            ])
        }
        let longWaveBytes = 44 + Int(TinyArcadeToneSynthesizer.sampleRate) * 2 * 2
        precondition(tonePlayer.cachedWaveCount == 5)
        precondition(tonePlayer.cachedWaveBytes == 5 * longWaveBytes)
        precondition(tonePlayer.waveSynthesisCount == 21)
        let paddleView = TinyArcadeIndexed2DView(
            frame: CGRect(x: 0, y: 0, width: 390, height: 844)
        )
        try paddleView.display(initialPaddle)
        var paddleMilliseconds: [Double] = []
        paddleMilliseconds.reserveCapacity(600)
        var sawPaddleTone = !paddleFrame.tones.isEmpty
        var paddleMaxSteps: UInt64 = 0
        var paddleMaxPages: UInt32 = 0
        for index in 1...600 {
            if index == 300 {
                let saved = try paddleRuntime.suspend()
                let resumed = try makePaddleRuntime()
                try resumed.resume(snapshot: saved)
                try paddleRuntime.close()
                paddleRuntime = resumed
            }
            let buttons: UInt32 = (index / 90).isMultiple(of: 2) ? 1 << 0 : 1 << 1
            let started = ProcessInfo.processInfo.systemUptime
            paddleFrame = try paddleRuntime.tickMedia(
                buttons: buttons,
                clockMilliseconds: UInt32(index * 16)
            )
            guard case let .indexed2D(decoded) = paddleFrame.renderFrame else {
                preconditionFailure("Paddle Guard changed render protocol")
            }
            try paddleView.display(decoded)
            paddleMilliseconds.append((ProcessInfo.processInfo.systemUptime - started) * 1_000)
            let stats = try paddleRuntime.lastExecutionStats()
            let extendedStats = try paddleRuntime.lastExecutionStatsV2()
            precondition(stats.lifecycle == .tick)
            precondition(stats.renderBytes == UInt32(paddleFrame.render.count))
            precondition(stats.audioBytes == UInt32(paddleFrame.audio.count))
            precondition(stats.wasmSteps > 0 && stats.wasmSteps <= 500_000)
            precondition(stats.memoryPages <= 17)
            precondition(extendedStats.lifecycle == stats.lifecycle)
            precondition(extendedStats.wasmSteps == stats.wasmSteps)
            precondition(extendedStats.peakCallDepth > 0 && extendedStats.peakCallDepth <= 512)
            precondition(extendedStats.peakActivationSlots > 0 && extendedStats.peakActivationSlots <= 1_048_576)
            paddleMaxSteps = max(paddleMaxSteps, stats.wasmSteps)
            paddleMaxPages = max(paddleMaxPages, stats.memoryPages)
            sawPaddleTone = sawPaddleTone || !paddleFrame.tones.isEmpty
        }
        paddleMilliseconds.sort()
        let paddleAverage = paddleMilliseconds.reduce(0, +) / Double(paddleMilliseconds.count)
        let paddleP95 = paddleMilliseconds[Int(Double(paddleMilliseconds.count - 1) * 0.95)]
        let paddleMaximum = paddleMilliseconds.last ?? 0
        precondition(sawPaddleTone, "Paddle Guard must emit gameplay feedback")
        precondition(paddleP95 < 8, "Paddle Guard simulator p95 exceeded 8 ms")
        print(
            "OK: Paddle Guard in iOS Simulator; "
                + String(
                    format: "600 frames avg=%.3fms p95=%.3fms max=%.3fms fuel=%llu pages=%u",
                    paddleAverage, paddleP95, paddleMaximum, paddleMaxSteps, paddleMaxPages
                )
        )
        try paddleRuntime.close()
    }
}
