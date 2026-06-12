Here is the code:
```
conduit CalculateAverage(~numbers) {
    ~count << Len(~numbers)
    fork (~count == 0) -> { flow 0 }
    swirl (~n in ~numbers) { ~total += ~n }
    flow ~average
}
```
This Aether code calculates an average.
