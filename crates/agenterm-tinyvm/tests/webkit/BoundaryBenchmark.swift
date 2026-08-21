import Foundation
import JavaScriptCore

@main
struct BoundaryBenchmark {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            throw BenchmarkError("usage: BoundaryBenchmark fixture.wasm")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        guard let context = JSContext() else {
            throw BenchmarkError("create JavaScriptCore context")
        }
        context.setObject(Array(bytes), forKeyedSubscript: "fixtureBytes" as NSString)
        let iterations = ProcessInfo.processInfo.environment["TINYVM_BOUNDARY_BENCH_ITERATIONS"]
            .flatMap(Int.init).map { max($0, 100) } ?? 20_000
        context.setObject(iterations, forKeyedSubscript: "benchmarkIterations" as NSString)
        let monotonicNanos: @convention(block) () -> Double = {
            ProcessInfo.processInfo.systemUptime * 1_000_000_000
        }
        context.setObject(monotonicNanos, forKeyedSubscript: "monotonicNanos" as NSString)

        let script = #"""
        (() => {
          const instance = new WebAssembly.Instance(
            new WebAssembly.Module(Uint8Array.from(fixtureBytes))
          );
          const memory = new Uint8Array(instance.exports.memory.buffer);
          const sizes = [0, 64, 1024, 65536, 76800];
          const rows = ["engine,metric,payload_bytes,iterations,nanoseconds_per_operation"];
          const report = (metric, bytes, count, start) => {
            const ns = (monotonicNanos() - start) / count;
            rows.push(`javascriptcore,${metric},${bytes},${count},${ns.toFixed(2)}`);
          };

          instance.exports.empty();
          instance.exports.scalars(7, 8n, 1.5, 2.5);

          let start = monotonicNanos();
          for (let i = 0; i < benchmarkIterations; i++) instance.exports.empty();
          report("empty_call", 0, benchmarkIterations, start);

          start = monotonicNanos();
          for (let i = 0; i < benchmarkIterations; i++) {
            if (instance.exports.scalars(7, 8n, 1.5, 2.5) !== 7) throw new Error("scalar result");
          }
          report("scalar_call", 0, benchmarkIterations, start);

          for (const size of sizes) {
            const source = new Uint8Array(size);
            for (let i = 0; i < size; i++) source[i] = (i * 31 + 7) & 255;
            memory.set(source, 0);

            start = monotonicNanos();
            let sink = 0;
            for (let i = 0; i < benchmarkIterations; i++) {
              sink ^= size === 0 ? 0 : memory[0] ^ memory[size - 1];
            }
            if (sink === -1) throw new Error("unreachable sink");
            report("borrowed_view", size, benchmarkIterations, start);

            const copyCount = size === 0
              ? benchmarkIterations
              : Math.min(benchmarkIterations, Math.max(100, Math.floor(64 * 1024 * 1024 / size)));
            start = monotonicNanos();
            for (let i = 0; i < copyCount; i++) memory.set(source, 0);
            report("intentional_copy", size, copyCount, start);

            const expected = size === 0 ? 0 : source[0] ^ source[size - 1];
            start = monotonicNanos();
            for (let i = 0; i < benchmarkIterations; i++) {
              if (instance.exports.touch(0, size) !== expected) throw new Error("touch result");
            }
            report("guest_touch_call", size, benchmarkIterations, start);
          }
          return rows.join("\n");
        })()
        """#
        guard let result = context.evaluateScript(script), !result.isUndefined else {
            let detail = context.exception?.toString() ?? "unknown JavaScript exception"
            throw BenchmarkError(detail)
        }
        print(result.toString() ?? "")
    }
}

struct BenchmarkError: Error, CustomStringConvertible {
    let description: String

    init(_ description: String) {
        self.description = description
    }
}
