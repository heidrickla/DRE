# Diagnosis pack — not yet written

Model Pack B, per plan section 19. Mechanical diagnosis from incomplete
observations:

```text
temperature        = high
vibration          = high
pressure           = normal
internal condition = unknown
```

Exercises evidence handling, inference, geometric classification, and —
most importantly — reasoning that must still reach a conclusion with a
property it cannot observe.

The `internal condition` field is the point of this pack. It should be
modelled as `UNOBSERVABLE` rather than `UNKNOWN`: no amount of further
evidence gathering will resolve it, and a reasoner that keeps waiting for
an observation that cannot arrive is a different failure from one that
concludes under uncertainty.

Blocked on phases 3 and 5.
