# Aether Translation

The following is the provided Python snippet translated into valid Aether (Release 6.3).

---

## Translation Notes

| Python construct | Aether equivalent | Rule |
|---|---|---|
| `def sum(a, b):` | `conduit Sum(~a, ~b)` | Conduit names are PascalCase (no prefix); parameters are wave identifiers (`~`) |
| `return (a + b)` | `flow (~a + ~b)` | `flow` is the return statement (§7.7) |
| `a = int(input(...))` | `~a << Drop(Input(...))` | `<<` is the Flow/Assign operator (§7.2); `Input` and `Drop` are intrinsic conduits (§3.5) |
| `f'Sum of {a} ...'` | `echo"Sum of ${~a} ..."` | Interpolated Echo prefixed with `echo`; expressions injected via `${...}` (§2.3.1) |

---

## Solution

```aether
conduit Sum(~a, ~b) {
    flow (~a + ~b)
}

~a << Drop(Input("1st number: "))
~b << Drop(Input("2nd number: "))

print(echo"Sum of ${~a} and ${~b} is ${Sum(~a, ~b)}")
```

The program uses top-level statements (no `conduit Main()` wrapper) so the interpreter executes them sequentially per §4.1.
