import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct ImportedMemoryOracle {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else { throw OracleError.usage }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let providerBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
        guard let context = JSContext() else { throw OracleError.context }
        var javascriptError: String?
        context.exceptionHandler = { _, exception in
            javascriptError = exception?.toString() ?? "unknown JavaScript exception"
        }
        context.setObject(Array(bytes), forKeyedSubscript: "hostBytes" as NSString)
        context.setObject(Array(providerBytes), forKeyedSubscript: "providerBytes" as NSString)
        let value = context.evaluateScript(
            """
            (() => {
              const module = new WebAssembly.Module(Uint8Array.from(hostBytes));
              const providerModule = new WebAssembly.Module(Uint8Array.from(providerBytes));
              const provider = new WebAssembly.Instance(providerModule);
              const ram = provider.exports.ram;
              const imports = {host: {ram}};
              const first = new WebAssembly.Instance(module, imports);
              const second = new WebAssembly.Instance(module, imports);
              if (first.exports.ram !== ram || second.exports.ram !== ram) return -1;
              const a = first.exports.run();
              const b = second.exports.run();
              new Uint8Array(ram.buffer)[0] = 70;
              const c = first.exports.run();
              const grown = first.exports.grow();
              const pages = second.exports.size();
              return a + b + c + grown * 10 + pages;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 516 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore standard imported-memory result=516")
    }
}
