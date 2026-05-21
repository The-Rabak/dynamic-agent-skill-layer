# Patterns and Backends

This reference covers orchestration patterns, backend selection, environment variables, and failure handling.

## Orchestration patterns

### Parallel specialists
Use when the work splits naturally by expertise and results can be synthesized by the leader.

### Pipeline
Use when stage ordering matters: research -> plan -> implement -> validate.

### Self-organizing swarm
Use only when workers should claim from a shared task list and the work is truly queue-shaped.

### Research plus implementation
Pair a research worker with an implementation worker when best-practice discovery should feed action quickly.

### Approval workflow
Use explicit plan approval or review gates when a leader must approve before execution continues.

### Coordinated multi-file refactor
Break work into ownership-safe regions, then reserve one synthesis pass for integration risk.

## Environment variables

Common environment concerns:
- team/runtime identifiers
- backend selection
- terminal integration hints
- path/tool availability for child workers

Keep backend-sensitive assumptions out of worker prompts when possible.

## Spawn backends

### in-process
Default and cheapest for invisible workers and fast coordination.

### tmux
Useful when you want visible panes and shell-native supervision.

### iterm2
macOS-only visual split-pane option.

### Forcing a backend
Override auto-detection only when you have a concrete operational need.

## Backend comparison

| Backend | Best for | Tradeoff |
|---|---|---|
| `in-process` | cheap, fast, low-overhead workers | low visibility |
| `tmux` | visible parallel panes | more setup and screen management |
| `iterm2` | macOS visual workflows | platform-specific |

## Error handling

### Common failures
- worker never returns
- teammate crashes
- dependency graph blocks progress
- wrong backend or missing terminal capability
- workers duplicate effort because ownership is unclear

### Recovery posture
- prefer direct reassignment over broad restart
- inspect inboxes and task state before guessing
- re-run the smallest broken unit
- shut down stale workers instead of layering on replacements blindly

### Debugging
Check:
- team config
- inbox content
- task ownership/state
- backend assumptions
- whether leader instructions were explicit enough
