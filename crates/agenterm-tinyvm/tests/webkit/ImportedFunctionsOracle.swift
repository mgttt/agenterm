import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct ImportedFunctionsOracle {
    static func main() throws {
        guard CommandLine.arguments.count == 4 else { throw OracleError.usage }
        let providerBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let consumerBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
        let relayBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3]))
        guard let context = JSContext() else { throw OracleError.context }
        var javascriptError: String?
        context.exceptionHandler = { _, exception in
            javascriptError = exception?.toString() ?? "unknown JavaScript exception"
        }
        context.setObject(Array(providerBytes), forKeyedSubscript: "providerBytes" as NSString)
        context.setObject(Array(consumerBytes), forKeyedSubscript: "consumerBytes" as NSString)
        context.setObject(Array(relayBytes), forKeyedSubscript: "relayBytes" as NSString)
        let value = context.evaluateScript(
            """
            (() => {
              const provider = new WebAssembly.Instance(
                new WebAssembly.Module(Uint8Array.from(providerBytes))
              );
              const consumer = new WebAssembly.Instance(
                new WebAssembly.Module(Uint8Array.from(consumerBytes)),
                {provider: {
                  add: provider.exports.add,
                  sub: provider.exports.sub,
                  mixed: provider.exports.mixed,
                  identity_ref: provider.exports.identity_ref
                }}
              );
              const relay = new WebAssembly.Instance(
                new WebAssembly.Module(Uint8Array.from(relayBytes)),
                {relay: {function: consumer.exports.reexport}}
              );
              return consumer.exports.run() * 100000
                + consumer.exports.tail() * 1000
                + relay.exports.run() * 10
                + consumer.exports.typed();
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 4242424 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore linked function result=4242424")
    }
}
