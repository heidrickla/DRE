# Navigation pack — not yet written

Model Pack C, per plan section 19. Grid pathfinding:

```text
start     = [1,1]
goal      = [20,20]
obstacles = [...]
hazards   = [...]
```

Exercises geometry, constraints, planning, and deterministic path
selection.

This is the pack most likely to expose a determinism bug, because grid
navigation generates large numbers of equal-cost paths. If frontier
ordering is not total, two runs will return different — equally optimal —
routes, and the `DecisionHash` will differ while every individual step
looks correct. Obstacles should be placed deliberately to create
equal-cost alternatives rather than avoiding them.

Hazards are the natural test for soft constraints: avoid unless there is
no other route.

Blocked on phases 3, 8, and 10.
