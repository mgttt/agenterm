import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct ImportedGlobalsOracle {
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
              const base = new WebAssembly.Global({value: "i32", mutable: false}, 3);
              const counter = new WebAssembly.Global({value: "i32", mutable: true}, 10);
              const imports = {host: {base, counter}};
              const first = new WebAssembly.Instance(module, imports);
              const a = first.exports.run();
              const second = new WebAssembly.Instance(module, imports);
              const b = second.exports.run();
              counter.value = 20;
              const c = first.exports.run();
              return a * 10000 + b * 100 + c;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 878897 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore standard imported-global result=878897")
    }
}
