# primitives — Q6: is the four-primitive floor stable?

Decisive experiment: [`plan/archive/design-primitive-completeness-experiment.md`](../../../plan/archive/design-primitive-completeness-experiment.md).
Numbers and verdict: [`RESULTS.md`](./RESULTS.md).

Adds three capabilities of divergent arg-count/shape — **memory-mapped file** (arity 7, no
layout), **directory traversal** (arity 2, reads a struct field), **socket bind** (arity 3,
writes struct fields) — using **only** Q0's four primitives (③ sym + ④ call + ①② memory), on
real Windows/x86_64. Measures whether ④'s arg ceiling is forced up (① step vs slope), whether
four primitives can reach struct layout (② Declare necessary?), the byte floor of a candidate
fifth primitive (③), and gives a revised completeness argument (④).

## One-line conclusion

**内核尺寸/arity/原语数稳定（Claim K 成立，④ 的 7→11 是一次性台阶不是斜率）；但 §1.1 的
"没有够不到的东西"(Claim R) 被证伪** —— `offsetof` 由四原语中的任何一条都产不出来，只能烘焙
(信任转移)或由宿主发布的 layout 经 ③+④ 取回。封闭清单 = **五条**（① memory ② execute ③ reach
④ call ⑤ declare），带**宿主条件星号**：⑤ 的完备性成立当且仅当宿主发布描述，否则退化为烘焙表。

## Files

```
main.rs      Win64 harness — runs A/B/C, prints ① arity table + ② layout classification
kernel4.rs   no_std: four primitives           (.text = 550 B)
kernel5.rs   no_std: four + declare            (.text = 732 B; declare = +182 B)
RESULTS.md   ①②③④ numbers + decision trace + deviations
```

## Reproduce

```powershell
cd research/dynamic-core/primitives ; mkdir out 2>$null ; copy main.rs out\
rustc --edition 2021 -O -A nonstandard_style main.rs -o out/harness.exe
cd out ; ./harness.exe
```

See `RESULTS.md` for the ③ byte-floor commands. Not hooked into the root workspace.
