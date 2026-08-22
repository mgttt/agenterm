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
              const laneOperations = [[1,0],[1,1],[2,0],[2,1],[2,2],[4,0],[4,1],[4,2],[8,0],[8,1],[8,2]];
              const laneVector = ([width, arithmetic]) => {
                const vector = new Array(16).fill(0);
                const laneMask = (1n << BigInt(width * 8)) - 1n;
                for (let start = 0; start < 16; start += width) {
                  let a = 0n;
                  let b = 0n;
                  for (let byte = 0; byte < width; byte++) {
                    a |= BigInt(left[start + byte]) << BigInt(byte * 8);
                    b |= BigInt(right[start + byte]) << BigInt(byte * 8);
                  }
                  let value = arithmetic === 0 ? a + b : arithmetic === 1 ? a - b : a * b;
                  value &= laneMask;
                  for (let byte = 0; byte < width; byte++) {
                    vector[start + byte] = Number((value >> BigInt(byte * 8)) & 0xffn);
                  }
                }
                return vector;
              };
              bytes.fill(0, 256, 432);
              instance.exports.lanes(0, 16, 256);
              laneOperations.map(laneVector).forEach((vector, operation) => vector.forEach((value, index) => {
                const actual = bytes[256 + operation * 16 + index];
                if (actual !== value) throw new Error(`lanes ${operation}:${index}=${actual}`);
              }));
              return `${added}|${subtracted}|mask=${any}|lanes=pass`;
            })()
            """
        )
        if let javascriptError { throw OracleError.javascript(javascriptError) }
        let result = value?.toString() ?? ""
        let expected = "32767,-32768,300,-300,32767,-32768,-5000,5000|20000,-20000,-100,100,32766,-32767,32767,-32768|mask=1,0|lanes=pass"
        guard result == expected else { throw OracleError.wrongResult(result) }
        print("OK: JavaScriptCore SIMD game kernels=\(result)")
    }
}
