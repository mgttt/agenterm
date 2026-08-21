import Foundation
import TinyArcade

private final class HostProfileFixtureURLProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var profile = Data()

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == "profile.tinyarcade.test"
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        guard let url = request.url else { return }
        let body: Data
        let mime: String
        switch url.path {
        case "/catalog-v1.json":
            body = Self.catalogData()
            mime = "application/json"
        case "/wasm/host-profile-v1.tahost":
            body = Self.profile
            mime = "application/vnd.tinyarcade.host-profile"
        default:
            respond(status: 404, mime: "text/plain", body: Data())
            return
        }
        respond(status: 200, mime: mime, body: body)
    }

    override func stopLoading() {}

    private func respond(status: Int, mime: String, body: Data) {
        guard let url = request.url else { return }
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

    static func catalogData() -> Data {
        Data(
            """
            {
              "schema_version": 1,
              "catalog_id": "com.partnernet.tinyarcade",
              "host_profile": {
                "file": "host-profile-v1.tahost",
                "length": \(profile.count),
                "sha256": "5613220ea8cad191992c2b7c38d3f2dc3e1960117ebfd9d43950b06f9f4ae23a"
              },
              "games": [{
                "game_id": "com.partnernet.profile-smoke",
                "game_version": "1.0.0",
                "title": "Profile Smoke",
                "summary": "Profile discovery only.",
                "cartridge": "profile-smoke-1.0.0.wasm",
                "abi_version": 1,
                "state_version": 1,
                "wasm_length": 8,
                "wasm_sha256": "\(String(repeating: "0", count: 64))",
                "signing_key_id": "profile-smoke",
                "signature": "\(Data(repeating: 0, count: 64).base64EncodedString())"
              }]
            }
            """.utf8
        )
    }
}

@main
struct TinyArcadeHostProfileCatalogSmoke {
    static func main() async throws {
        let expected = try TinyArcadeHostProfileV1.appBuild()
        HostProfileFixtureURLProtocol.profile = expected.encoded
        let root = URL(string: "https://profile.tinyarcade.test/")!
        let wasmRoot = root.appendingPathComponent("wasm", isDirectory: true)
        let catalogData = HostProfileFixtureURLProtocol.catalogData()
        let catalog = try TinyArcadeCatalogV1.decode(
            catalogData,
            cartridgeBaseURL: wasmRoot
        )
        let metadata = catalog.hostProfile!
        precondition(metadata.length == UInt64(expected.encoded.count))
        precondition(
            metadata.fileURL.absoluteString
                == "https://profile.tinyarcade.test/wasm/host-profile-v1.tahost"
        )

        let traversal = Data(
            String(decoding: catalogData, as: UTF8.self)
                .replacingOccurrences(
                    of: "host-profile-v1.tahost",
                    with: "../host-profile-v1.tahost"
                ).utf8
        )
        do {
            _ = try TinyArcadeCatalogV1.decode(traversal, cartridgeBaseURL: wasmRoot)
            preconditionFailure("catalog profile traversal must fail")
        } catch let error as TinyArcadeCatalogDecodeError {
            precondition(error == .invalidDocument)
        }

        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [HostProfileFixtureURLProtocol.self]
        let transport = TinyArcadeHTTPSClientV1(
            configuration: configuration,
            timeoutInterval: 5
        )
        let fetchedCatalog = try await transport.fetchCatalog(
            at: root.appendingPathComponent("catalog-v1.json"),
            cartridgeBaseURL: wasmRoot
        )
        let published = try await transport.fetchHostProfile(
            fetchedCatalog.hostProfile!,
            matching: expected
        )
        precondition(expected.matchesPublishedBytes(published))

        let mismatched = try TinyArcadeHostProfileV1.appBuild { config in
            config.max_steps -= 1
        }
        do {
            _ = try await transport.fetchHostProfile(metadata, matching: mismatched)
            preconditionFailure("a different app-build profile must fail")
        } catch let error as TinyArcadeHTTPError {
            precondition(error == .hostProfileMismatch)
        }
        print("OK: catalog TAH1 discovery -> bounded HTTPS -> exact local app-profile match")
    }
}
