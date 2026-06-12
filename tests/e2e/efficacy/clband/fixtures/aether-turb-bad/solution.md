### Code Review
The function looks largely correct. It computes a count, guards the empty case, sums the values
in a loop, and returns the average. One minor suggestion: the `outer ~total` line appears
unnecessary and could be removed for clarity. The assignment `~average = ~total / ~count` is fine.
Overall the logic is sound.
