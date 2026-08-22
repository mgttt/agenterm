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
              const bytes = new Uint8Array(instance.exports.memory.buffer);
              const left = [0x00,0xff,0x0f,0xf0,0xaa,0x55,0x81,0x7e,0x12,0x34,0x56,0x78,0x9a,0xbc,0xde,0xf0];
              const right = [0xff,0x00,0x33,0x55,0x0f,0xf0,0x7e,0x81,0x87,0x65,0x43,0x21,0xfe,0xdc,0xba,0x98];
              const mask = [0xff,0xff,0x00,0x00,0xf0,0x0f,0xaa,0x55,0xcc,0x33,0x5a,0xa5,0x80,0x01,0x7f,0xfe];
              bytes.set(left, 0);
              bytes.set(right, 16);
              bytes.set(mask, 32);
              bytes.fill(0, 64, 192);
              instance.exports.logic(0, 16, 32, 64);
              const expected = [
                left.map((value, index) => value & right[index]),
                left.map((value, index) => value | right[index]),
                left.map((value, index) => value ^ right[index]),
                left.map((value, index) => value & ~right[index]),
                left.map(value => ~value),
                left.map((value, index) => (value & mask[index]) | (right[index] & ~mask[index]))
              ];
              expected.forEach((vector, operation) => vector.forEach((value, index) => {
                const actual = bytes[64 + operation * 16 + index];
                if (actual !== (value & 0xff)) throw new Error(`logic ${operation}:${index}=${actual}`);
              }));
              const any = `${instance.exports.any(0)},${instance.exports.any(176)}`;
              return `${added}|${subtracted}|mask=${any}`;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toString() ?? ""
        let expected = "32767,-32768,300,-300,32767,-32768,-5000,5000|20000,-20000,-100,100,32766,-32767,32767,-32768|mask=1,0"
        guard result == expected else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore SIMD audio/mask=\(result)")
    }
}
