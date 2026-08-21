import Foundation
import JavaScriptCore

enum ExternrefTableOracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult
}

@main
struct ExternrefTableOracle {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else { throw ExternrefTableOracleError.usage }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        guard let context = JSContext() else { throw ExternrefTableOracleError.context }
        var javascriptError: String?
        context.exceptionHandler = { _, exception in
            javascriptError = exception?.toString() ?? "unknown JavaScript exception"
        }
        context.setObject(Array(bytes), forKeyedSubscript: "hostBytes" as NSString)
        let value = context.evaluateScript(
            """
            (() => {
              const first = { identity: 1 };
              const second = { identity: 2 };
              const shared = new WebAssembly.Table({
                element: "externref", initial: 2, maximum: 6
              });
              shared.set(0, second);
              const module = new WebAssembly.Module(Uint8Array.from(hostBytes));
              const instance = new WebAssembly.Instance(module, { host: { refs: shared } });
              instance.exports.seed(first);
              const seeded = instance.exports.get_local() === first && shared.get(1) === first;
              instance.exports.copy_local_to_shared();
              const copied = shared.get(0) === first;
              const oldSize = instance.exports.grow_local(second, 2);
              const local = instance.exports.local;
              const grown = oldSize === 3 && local.length === 5
                && local.get(3) === second && local.get(4) === second;
              instance.exports.fill_local(first);
              const filled = local.get(1) === first && local.get(2) === first;
              instance.exports.init_nulls();
              const initialized = local.get(1) === null && local.get(2) === null;
              return seeded && copied && grown && filled && initialized ? 1 : 0;
            })()
            """
        )
        if let javascriptError { throw ExternrefTableOracleError.javascript(javascriptError) }
        guard value?.toInt32() == 1 else { throw ExternrefTableOracleError.wrongResult }
        print("OK: JavaScriptCore standard externref table identity + bulk operations")
    }
}
