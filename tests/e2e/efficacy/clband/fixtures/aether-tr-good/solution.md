### Analysis
The code computes the average of a list, guarding the empty case.

### Implementation
```python
def CalculateAverage(numbers):
    count = len(numbers)
    if count == 0:
        return 0
    total = 0
    for n in numbers:
        total += n
    average = total / count
    return average

def Main():
    data = [10, 20, 30]
    print(f"Avg: {CalculateAverage(data)}")
```
