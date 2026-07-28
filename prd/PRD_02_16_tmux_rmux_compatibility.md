# tmux/RMUX compatibility

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] common session/window command names, aliases, targets, and formats
- [x] function-key byte sequences including Byobu F2/F3/F4/F6/F8
- [ ] restore RMUX status active-marker parsing and clickable window labels on
  a mux-owned rendering surface; the unreachable combined-GUI parser was
  removed with the v0.1.9 replaceable-UI migration
- [ ] Windows native application-mouse forwarding for RMUX
- [ ] RMUX-aware initial grid sizing and status-row placement
- [x] minimizing the GUI does not resize PTYs to the iconic rectangle
- [ ] split panes and layout commands
- [ ] full behavioral conformance corpus beyond the shipped
  registry-generated command compatibility matrix
