# tinyvm cross-boundary benchmark

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: executable development evidence; timings are not release thresholds

`smoke-boundary-benchmark.sh` compiles and validates one standard Wasm fixture,
then executes the exact bytes through release tinyvm and public
JavaScriptCore. Each engine must publish the same 32 metric/payload dimensions
with valid positive observations. The gate compares the matrix, not timing
rankings.

The benchmark separates these operations:

- empty host-to-guest call;
- mixed scalar host-to-guest call;
- host borrowed view of live linear memory;
- intentional host-to-memory copy;
- constant-work guest memory touch;
- guest-to-host import with the legacy memory-zero slice;
- guest-to-host import with `WasmHostMemories::memory(0)`;
- the same selected-memory import with an explicit copy into a preallocated
  host buffer.

Payload points are 0, 64, 1,024, 65,536 and 76,800 bytes. Copy rows cap their
iteration count by an aggregate 64 MiB transfer budget; constant-work view/call
rows retain the full requested iteration count. All paths verify the same first
and last byte result so an optimizer cannot replace them with an unchecked
empty loop.

The shared oracle uses memory zero because the tested public `JSContext`
rejects a module declaring two memories. This is recorded capability evidence,
not a reason to narrow tinyvm: independent WABT/tinyvm tests cover standard
memory indexes above zero and aliased imported memories. The shared benchmark's
purpose is to compare the overhead of legacy versus indexed host access on the
same allocation.

On the 2026-08-22 Apple Silicon development run with 20,000 call/view
iterations, tinyvm's selected-memory view stayed close to its legacy
memory-zero view at every payload point, while the explicit 64 KiB and 76,800-
byte copies showed the expected size-dependent increase. JavaScriptCore showed
the same qualitative separation. These numbers are observations for design
decisions, not portable promises or CI pass/fail ceilings.

Run:

```sh
./crates/agenterm-tinyvm/smoke-boundary-benchmark.sh
```

Set `TINYVM_BOUNDARY_BENCH_ITERATIONS` for exploratory runs; the script floors
it at 100 and still requires the complete 32-row matrix.
