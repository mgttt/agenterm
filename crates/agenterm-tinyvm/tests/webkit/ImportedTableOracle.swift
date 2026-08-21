import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(Int32)
}

@main
struct ImportedTableOracle {
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
              const dispatch = new WebAssembly.Table({initial: 1, maximum: 3, element: "anyfunc"});
              const imports = {host: {dispatch}};
              const first = new WebAssembly.Instance(module, imports);
              if (first.exports.dispatch !== dispatch) return -1;
              const a = first.exports.run();
              const second = new WebAssembly.Instance(module, imports);
              if (second.exports.dispatch !== dispatch) return -2;
              const b = first.exports.run();
              const c = second.exports.run();
              return a + b + c;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toInt32() ?? Int32.min
        guard result == 4 else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore imported-table sibling dispatch result=4")
    }
}
