**Turbulence Alert**
- **Cause:** The line `~average = ~total / ~count` uses `=`, but Aether assignment is `<<`.
- **Fix:** Replace `=` with `<<`.
- **Corrected Code:**
```aether
conduit CalculateAverage(~numbers) {
    ~count << Len(~numbers)
    fork (~count == 0) -> { flow 0 }
    ~total << 0
    swirl (~n in ~numbers) {
        outer ~total
        ~total += ~n
    }
    ~average << ~total / ~count
    flow ~average
}
```
