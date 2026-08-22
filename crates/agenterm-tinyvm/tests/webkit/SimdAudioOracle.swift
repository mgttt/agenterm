import Foundation
import JavaScriptCore

enum OracleError: Error {
    case usage
    case context
    case javascript(String)
    case wrongResult(String)
}

@main
struct SimdAudioOracle {
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
              const instance = new WebAssembly.Instance(module, {});
              const samples = new Int16Array(instance.exports.memory.buffer);
              samples.set([30000,-30000,100,-100,32767,-32768,20000,-20000], 0);
              samples.set([10000,-10000,200,-200,1,-1,-25000,25000], 8);
              instance.exports.mix(0, 16, 32);
              const added = Array.from(samples.slice(16, 24)).join(',');
              instance.exports.subtract(0, 16, 32);
              const subtracted = Array.from(samples.slice(16, 24)).join(',');
              return `${added}|${subtracted}`;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toString() ?? ""
        let expected = "32767,-32768,300,-300,32767,-32768,-5000,5000|20000,-20000,-100,100,32766,-32767,32767,-32768"
        guard result == expected else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore SIMD audio add/sub=\(result)")
    }
}
