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
        guard CommandLine.arguments.count == 4 else { throw OracleError.usage }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let providerBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
        let linkedConsumerBytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3]))
        guard let context = JSContext() else { throw OracleError.context }
        var javascriptError: String?
        context.exceptionHandler = { _, exception in
            javascriptError = exception?.toString() ?? "unknown JavaScript exception"
        }
        context.setObject(Array(bytes), forKeyedSubscript: "hostBytes" as NSString)
        context.setObject(Array(providerBytes), forKeyedSubscript: "providerBytes" as NSString)
        context.setObject(Array(linkedConsumerBytes), forKeyedSubscript: "linkedConsumerBytes" as NSString)
        let value = context.evaluateScript(
            """
            (() => {
              const module = new WebAssembly.Module(Uint8Array.from(hostBytes));
              const providerModule = new WebAssembly.Module(Uint8Array.from(providerBytes));
              const provider = new WebAssembly.Instance(providerModule);
              const dispatch = provider.exports.dispatch;
              const linkedConsumerModule = new WebAssembly.Module(Uint8Array.from(linkedConsumerBytes));
              const linkedConsumer = new WebAssembly.Instance(linkedConsumerModule, {host: {dispatch}});
              if (linkedConsumer.exports.run() !== 42) return -3;
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
