import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct TypedHostOracle {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else { throw OracleError.usage }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        guard let context = JSContext() else { throw OracleError.context }
        var javascriptError: String?
        context.exceptionHandler = { _, exception in
            javascriptError = exception?.toString() ?? "unknown JavaScript exception"
        }
        context.setObject(Array(bytes), forKeyedSubscript: "hostBytes" as NSString)
        let value = context.evaluateScript(
            """
            (() => {
              const module = new WebAssembly.Module(Uint8Array.from(hostBytes));
              const imports = { host: { mix: (a, b, c) => [c + 2, a + 2n, b + 2] } };
              const instance = new WebAssembly.Instance(module, imports);
              const result = instance.exports.run();
              return result[0] === 4.5 && result[1] === 42n && result[2] === 3.5 ? 143 : -1;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 143 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore standard typed host import result=143")
    }
}
