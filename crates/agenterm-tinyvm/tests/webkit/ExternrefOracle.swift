import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32, Int32)
}

@main
struct ExternrefOracle {
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
              const token = { identity: 42 };
              const module = new WebAssembly.Module(Uint8Array.from(hostBytes));
              const instance = new WebAssembly.Instance(module, {
                host: {
                  source: () => token,
                  sink: value => value === token ? 1 : 0,
                },
              });
              const nullResult = instance.exports.null_is_null();
              const nonnullResult = instance.exports.host_is_not_null();
              const roundtripResult = instance.exports.roundtrip();
              const globalResult = instance.exports.saved.value === token ? 1 : 0;
              instance.exports.saved.value = null;
              const clearedResult = instance.exports.saved.value === null ? 1 : 0;
              return [nullResult, nonnullResult, roundtripResult, globalResult, clearedResult];
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let nullResult = value?.objectAtIndexedSubscript(0)?.toInt32() ?? Int32.min
        let nonnullResult = value?.objectAtIndexedSubscript(1)?.toInt32() ?? Int32.min
        let roundtripResult = value?.objectAtIndexedSubscript(2)?.toInt32() ?? Int32.min
        let globalResult = value?.objectAtIndexedSubscript(3)?.toInt32() ?? Int32.min
        let clearedResult = value?.objectAtIndexedSubscript(4)?.toInt32() ?? Int32.min
        guard nullResult == 1, nonnullResult == 1, roundtripResult == 1,
              globalResult == 1, clearedResult == 1 else {
            throw OracleError.wrongResult(nullResult, roundtripResult)
        }
        print("OK: JavaScriptCore standard externref null + identity roundtrip")
    }
}
