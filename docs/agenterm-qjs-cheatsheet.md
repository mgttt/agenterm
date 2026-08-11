# qjs condensed manual

Practical reference for writing `scripts/qjs/**.js`. Companion to
[`docs/agenterm-rh-cheatsheet.md`](agenterm-rh-cheatsheet.md). Engine roadmap
context lives in `plan/plan-v0.1.16.md` §1 Rh.

qjs is the QuickJS-ng backend, capability-aligned with rh at the *host boundary*
but nothing like it in language terms. The two failure modes are opposite:

- **rh** punishes you for writing something outside its lowerable subset.
- **qjs** accepts all of modern JavaScript, then fails because **the capability
  you assumed exists does not exist at all**.

Read §1 before anything else.

---

## 1. The entire host surface (read this first)

A qjs script can reach **four** host primitives and nothing more:

```js
__host.fleet_call(operation_id, params_json) // -> result_json (string)
__host.args_len()                            // -> number
__host.arg(index)                            // -> string
__host.print(text)                           // also installed as global print()
```

Plus whatever QuickJS itself provides: `JSON`, `Math`, `String`, `Array`,
`Map`/`Set`, `RegExp`, `Promise`, `BigInt`, template literals, destructuring,
classes, `async`/`await`, ES modules.

**There is no filesystem, no process spawning, no network, no clock beyond
`Date`, no `std`/`os` module, no `require`.** QuickJS's `quickjs-libc` (`std.*`,
`os.*`) is *not* linked in. Writing `os.readFile`, `require('fs')`, or
`std.open` will fail — those are not "missing bindings we should add", they are
outside the current design. Everything effectful goes through
`__host.fleet_call`.

Contrast with rh, which ships a large `std::fs` / `std::process` / `std::net`
surface. **Do not port an rh script to qjs by translating its syntax** — most rh
scripts are built on capabilities qjs does not have.

## 2. Skeleton

```js
// One-line purpose.

function entry() {
  const count = __host.args_len();
  if (count < 1) {
    print("expected: REPO");
    return 1;
  }
  const repo = __host.arg(0);
  print(`PASS my-script ${repo}`);
  return 0;
}
```

- A top-level `function entry()` is **required**. `eval_entry` fails closed if it
  is missing rather than falling back to running the file as a top-level script.
- `entry()`'s return value is reported through `JSON.stringify`, so it is not
  restricted to an integer the way rh's i64 entry ABI is. Return `0` for success
  by convention.
- `print()` is the stdout channel; it is captured, not written directly.
- Arguments are read one at a time by index, not from an array.

## 3. Two execution modes, chosen automatically

`crates/agenterm-qjs/src/module_sniff.rs` looks for **top-level `import` or
`export`** and routes accordingly:

| Script contains | Mode | Semantics |
|-----------------|------|-----------|
| top-level `import` / `export` | **module** | real ES modules, root-confined resolver |
| neither | **classical** | `entry()` looked up on `globalThis` |

Dynamic `import()` expressions deliberately do **not** trigger module mode —
they are legal in classical scripts too.

The consequence that bites: in classical mode `entry()` must be reachable from
`globalThis`, so a top-level `const entry = ...` will not be found — use
`function entry()`.

## 4. Module imports — resolved relative to the importing file

```js
import { fleet } from "./lib/fleet.js";
import { helper } from "../shared/helper.js";
```

- **qjs resolves relative to the importing file's directory** (`./foo.js`), the
  ES-module-idiomatic convention. **rh resolves relative to the project root.**
  This is the single most likely thing to get wrong when switching engines.
- Include the `.js` extension. rh omits its `.rh` extension; qjs does not.
- Resolution is confined to the project root by
  `crates/agenterm-qjs/src/module_resolver.rs`. `rquickjs`'s own
  `FileResolver` does not clamp to a root — that module is the actual security
  boundary, so absolute specifiers and `..` escapes are rejected there.
- `pack build` and `qualify` require `--project-root` explicitly for module-mode
  scripts, the same convention as `check`.

### State of `scripts/qjs/lib/fleet.js`

It exists as a line-for-line port of `scripts/lua/lib/fleet.lua` (same
`operation_id` strings and params shapes, so the same Fleet operation is
produced regardless of engine). **It currently has no `export` statement**, and
no qjs script in the repo imports it yet. Before importing it, add an export
(`export const fleet = ...`) — a top-level `const` in a separate classical
script is not visible to the importer. Its `call()` wrapper is the pattern worth
copying:

```js
function call(opId, params) {
  const resultJson = __host.fleet_call(opId, params === undefined ? "{}" : params);
  try {
    return JSON.parse(resultJson);
  } catch (_err) {
    return resultJson;    // not every operation returns JSON
  }
}
```

Note `fleet_call` takes and returns **strings**; you serialize and parse at the
boundary yourself.

## 5. CLI

```bash
agenterm qjs check <file.js> [--project-root DIR]
agenterm qjs eval <file.js> [--project-root DIR]
agenterm qjs run <file.js> [--project-root DIR] [-- <args>...]
agenterm qjs hash <file.js>
agenterm qjs pack build <file.js> --dir <out> [--project-root DIR]
agenterm qjs pack load <dir>
agenterm qjs qualify <file.js> --dir <out> [--project-root DIR]
agenterm qjs check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]
agenterm qjs corpus-scan [--dir <dir>]
agenterm qjs run-smoke <pack-dir>
agenterm qjs version
```

`agenterm-qjs` also exists as a standalone binary with the same verbs.

**`agenterm qjs task ...` is a stub.** qjs scripts cannot be wired into
`agenterm.tasks.json` gates yet, so there is no qjs equivalent of
`agenterm rh task run <id>`.

### What `check` does and does not prove

`check` calls QuickJS's `Module::declare`: it parses, compiles, and resolves
module-level `import`/`export` structure, and catches syntax errors — but it
**does not evaluate top-level statements**. A script whose only effect is a
top-level side effect will pass `check` with that effect never applied. Unlike
rh, there is no subset layer, so `check` cannot tell you "this construct will
not lower" — that category of error does not exist here.

## 6. Packs

`pack build` on a module-mode script produces a **self-contained multi-file
pack**: it copies the entire static import graph into the pack directory with
its own manifest schema (distinct from the single-file pack format), so
`pack load` never needs `--project-root` again.

The manifest hashes the whole graph, not just the entry. This was a deliberate
fix: changing an imported file's content left the entry module's own serialized
bytecode byte-identical, so hashing only the entry would have missed the change.

## 7. Traps

1. **Assuming rh's stdlib.** See §1. No fs, no process, no net.
2. **`const entry = () => {}` in classical mode.** Not on `globalThis`; use
   `function entry()`.
3. **Root-relative import specifiers.** `import "scripts/qjs/lib/fleet.js"` is
   rh's convention, not qjs's. Use `./lib/fleet.js`.
4. **Omitting `.js`.** Required here.
5. **Forgetting `--project-root` for module-mode `pack build`/`qualify`.**
6. **Expecting `fleet_call` to return an object.** It returns a string; some
   operations return non-JSON. Parse defensively (§4).
7. **Capturing `Ctx` in a host closure (Rust side, if you extend the binding).**
   Every bound closure must take its `Ctx<'js>` as a per-call parameter.
   Capturing a `ctx.clone()` creates a reference cycle QuickJS-ng's GC cannot
   collect and aborts the **whole process** with
   `Assertion failed: list_empty(&rt->gc_obj_list)`. This was found by crashing,
   not by inspection — see the module doc in `crates/agenterm-qjs/src/host.rs`.

## 8. Why the host boundary looks like lua's

`__host.{fleet_call,args_len,arg,print}` is intentionally the same shape and the
same global name as `agenterm_lua`'s `inject_host_table`, so a qjs script can be
a near-line-for-line port of its lua counterpart instead of a reinvention. That
mitigates parallel-spec drift between the two interpreted engines. rh's fleet
binding differs in kind — native codegen calling a C ABI host function — which
is an AOT mechanism, not part of capability alignment.
