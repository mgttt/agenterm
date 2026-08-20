# Research provenance and clean-room boundary

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [ ] `..\moltbaby` `bin/mux` and mapp `brain`/`flow` are research inputs
  only while their source files lack a definitive reusable license grant
- [ ] `..\moltbaby` `skills/mcu` (`bin/mcu`) is the living desktop-bridge lab
  for [28 agenterm-cu](PRD_02_28_agenterm_cu.md): command-set and layering
  lessons, not a TypeScript/Python transplant. Named window placement in cu
  (PRD 32, Spectacle catalog) is one landed slice. When `agenterm-cu` and
  AgenTerm are mature enough to replace that skill, `skills/mcu` archives and
  agents depend on this product line. Direct reuse of moltbaby source still
  requires the license/provenance rules below.
- [ ] AgenTerm may record public behavior, architectural lessons, test
  vectors created independently, and rejected approaches, but copies no
  source, comments, identifiers, documentation prose, or non-public fixture
  data from that repository
- [ ] implementation starts from AgenTerm's PRD and public contracts in a
  clean-room pass; provenance is recorded per imported dependency or
  externally derived compatibility fixture
- [ ] direct reuse requires an explicit compatible license and provenance
  review first; a placeholder or absent license is treated as no permission
