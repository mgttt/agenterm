import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct ResourceExportsOracle {
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
              const instance = new WebAssembly.Instance(
                new WebAssembly.Module(Uint8Array.from(hostBytes))
              );
              if (!(instance.exports.dispatch instanceof WebAssembly.Table)) return -1;
              if (!(instance.exports.ram instanceof WebAssembly.Memory)) return -2;
              if (instance.exports.dispatch.length !== 2) return -3;
              if (instance.exports.fixed.value !== 9n) return -4;
              new Uint8Array(instance.exports.ram.buffer)[1] = 66;
              instance.exports.counter.value = 11;
              return instance.exports.read();
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 76 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore standard resource-export result=76")
    }
}
