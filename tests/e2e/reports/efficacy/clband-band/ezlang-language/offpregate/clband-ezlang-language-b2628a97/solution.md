# EZLang Program Explanation: `timeChecker.ez`

## What the Program Does

This program repeatedly checks the current time every 30 minutes and records each check in a list. Once the current time reaches or passes 5:30 PM (17:30:00), it stops looping and prints every recorded timestamp.

---

## Line-by-Line Breakdown

### Setup

```ez
var times = []
var targetTime = "17:30:00"
```

- `times` is an empty list that will accumulate every timestamp the loop records.
- `targetTime` is a string holding the cutoff time in 24-hour `HH:MM:SS` format.

---

### `timeToSeconds` Function

```ez
func timeToSeconds with timeStr = {
    var parts = split with timeStr and ":"
    var hours = toNumber with 0 in parts
    var minutes = toNumber with 1 in parts
    var seconds = toNumber with 2 in parts
    return hours * 3600 + minutes * 60 + seconds
}
```

This converts a time string like `"17:30:00"` into a total number of seconds since midnight, making two times easy to compare numerically.

| Step | What happens |
|------|-------------|
| `split with timeStr and ":"` | Breaks the string on `":"`, producing a list: `["17", "30", "00"]` |
| `toNumber with 0 in parts` | Reads index 0 from the list and converts the string `"17"` to the number `17` |
| `hours * 3600 + minutes * 60 + seconds` | Converts everything to seconds: `17×3600 + 30×60 + 0 = 63000` |

**Relevant global functions (see Global Functions reference):**

- **`split`** — String Utilities section — splits a string by a delimiter and returns a list of substrings.
- **`toNumber`** — Type Conversion section — parses a string or value into a numeric type.
- **`in`** (index accessor) — List & Collection Operations section — retrieves an element from a list by zero-based index.

---

### `getTimePart` Function

```ez
func getTimePart = {
    var fullTime = now
    var parts = split with fullTime and " "
    return 1 in parts
}
```

`now` returns a full datetime string, for example `"2023-10-05 14:30:00"`. This function:

1. Captures the current datetime.
2. Splits on the space character, yielding `["2023-10-05", "14:30:00"]`.
3. Returns index `1` — the time-only portion `"14:30:00"`.

**Relevant global functions:**

- **`now`** — Date & Time section — returns the current system datetime as a formatted string (`"YYYY-MM-DD HH:MM:SS"`).
- **`split`** — String Utilities section (same as above).

---

### Pre-loop Calculation

```ez
var targetSeconds = timeToSeconds with targetTime
var shouldContinue = true
```

Converts the target time string once into seconds (`63000`) before the loop begins so the comparison is an integer check every iteration rather than a string comparison.

---

### The `while` Loop

```ez
while shouldContinue {
    var currentTime = now
    print with currentTime
    set times = times + [currentTime]

    var timePart = getTimePart
    var currentSeconds = timeToSeconds with timePart
    if currentSeconds greater than or equals targetSeconds {
        set shouldContinue = false
    } else {
        sleep with 1800
    }
}
```

Each pass through the loop:

1. **Captures** the current time string with `now`.
2. **Prints** it immediately with `print`.
3. **Appends** the timestamp to the `times` list by concatenating `times + [currentTime]` — wrapping the single value in `[]` makes it a one-element list before the concatenation.
4. **Extracts** just the time portion via `getTimePart`.
5. **Converts** that to seconds and compares against `targetSeconds`.
6. If it has reached or passed 17:30:00, sets `shouldContinue = false` to exit the loop.
7. Otherwise, **sleeps** for 1800 seconds (30 minutes) before the next check.

**Relevant global functions:**

- **`print`** — I/O section — writes a value to standard output followed by a newline.
- **`sleep`** — Control & Timing section — pauses execution for the given number of seconds.
- **`now`** — Date & Time section (same as above).

---

### Post-loop Output

```ez
print with "All checked times:"
for time in times {
    print with time
}
```

After the loop exits, the program iterates over every element in `times` with a `for...in` loop and prints each stored timestamp.

---

## Global Functions Quick Reference

| Function | Documentation Section | Purpose |
|----------|-----------------------|---------|
| `now` | Date & Time | Returns current datetime string |
| `split` | String Utilities | Splits a string on a delimiter |
| `toNumber` | Type Conversion | Parses a string into a number |
| `print` | I/O | Prints a value to stdout |
| `sleep` | Control & Timing | Pauses execution N seconds |

---

## Writing Output to a Text File

EZLang provides file-writing through the **`writeFile`** and **`appendFile`** global functions (I/O — File Operations section).

- **`writeFile with path and content`** — creates or overwrites the file at `path` with `content`.
- **`appendFile with path and content`** — appends `content` (followed by a newline) to the file at `path`, creating it if it does not exist.

To save every recorded time and the final summary to `checked_times.txt`, replace the `print` calls at the end and add file writes inside the loop:

```ez
// timeChecker.ez  (with file output)
var times = []
var targetTime = "17:30:00"
var outputFile = "checked_times.txt"

func timeToSeconds with timeStr = {
    var parts = split with timeStr and ":"
    var hours = toNumber with 0 in parts
    var minutes = toNumber with 1 in parts
    var seconds = toNumber with 2 in parts
    return hours * 3600 + minutes * 60 + seconds
}

func getTimePart = {
    var fullTime = now
    var parts = split with fullTime and " "
    return 1 in parts
}

// Clear / create the file with a header before the loop starts
writeFile with outputFile and "Time check log\n"

var targetSeconds = timeToSeconds with targetTime
var shouldContinue = true

while shouldContinue {
    var currentTime = now
    print with currentTime
    appendFile with outputFile and currentTime  // Write each timestamp as it is recorded
    set times = times + [currentTime]

    var timePart = getTimePart
    var currentSeconds = timeToSeconds with timePart
    if currentSeconds greater than or equals targetSeconds {
        set shouldContinue = false
    } else {
        sleep with 1800
    }
}

// Append the summary section
appendFile with outputFile and "---"
appendFile with outputFile and "All checked times:"
for time in times {
    print with time
    appendFile with outputFile and time
}
```

### Key changes explained

| Change | Why |
|--------|-----|
| `writeFile with outputFile and "Time check log\n"` | Runs once before the loop; creates the file fresh (or clears it) and writes a header so you do not accumulate runs from previous executions. |
| `appendFile with outputFile and currentTime` inside the loop | Writes each timestamp to disk immediately as it is recorded, so the file is up to date even if the program is interrupted mid-run. |
| `appendFile with outputFile and time` inside the `for` loop | Appends the same summary the `print` statements produce, giving the file a complete record identical to what was shown on screen. |

After the program finishes, `checked_times.txt` will contain every timestamp on its own line, prefixed by the header, and followed by the `---` separator and the summary list.
