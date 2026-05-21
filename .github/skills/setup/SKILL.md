---
name: setup
description: Configure which review agents run for your project. Auto-detects stack and writes compound-engineering.local.md.
model: gpt-5.3-codex
---

# Compound Engineering Setup

Interactive setup for `compound-engineering.local.md` -- configures which agents run during `/workflows-review` and `/workflows-work`, plus the visible Ralph/TDD defaults that planning and execution inherit.

## Step 1: Check Existing Config

Read `compound-engineering.local.md` in the project root. If it exists, display current settings summary (including the TDD contract) and use AskUserQuestion:

```
question: "Settings file already exists. What would you like to do?"
header: "Config"
options:
  - label: "Reconfigure"
    description: "Run the interactive setup again from scratch"
  - label: "View current"
    description: "Show the file contents, then stop"
  - label: "Cancel"
    description: "Keep current settings"
```

If "View current": read and display the file, then stop.
If "Cancel": stop.

## Step 2: Detect and Ask

Auto-detect the project stack by checking for common project indicators:

```bash
# Detect project stack from root files
if [ -f "artisan" ] && [ -f "composer.json" ]; then
  echo "laravel"
elif [ -f "Cargo.toml" ]; then
  echo "rust"
elif [ -f "nest-cli.json" ] || grep -q '"@nestjs/core"' package.json 2>/dev/null; then
  echo "nestjs"
elif [ -f "pom.xml" ] || [ -f "build.gradle" ] || [ -f "build.gradle.kts" ] || [ -d "src/main/java" ]; then
  echo "java"
elif [ -f "nuxt.config.ts" ] || [ -f "nuxt.config.js" ]; then
  echo "nuxt"
elif [ -f "next.config.ts" ] || [ -f "next.config.js" ] || [ -f "next.config.mjs" ]; then
  echo "nextjs"
elif [ -f "angular.json" ]; then
  echo "angular"
elif [ -f "vite.config.ts" ] && grep -q '"vue"' package.json 2>/dev/null; then
  echo "vue"
elif [ -f "vite.config.ts" ] && grep -q '"react"' package.json 2>/dev/null; then
  echo "react"
elif [ -f "composer.json" ]; then
  echo "php"
elif [ -f "tsconfig.json" ]; then
  echo "typescript"
elif [ -f "package.json" ]; then
  echo "javascript"
elif [ -f "pyproject.toml" ] || [ -f "requirements.txt" ] || [ -f "setup.py" ]; then
  echo "python"
elif [ -f "go.mod" ]; then
  echo "go"
else
  echo "general"
fi
```

Use AskUserQuestion:

```
question: "Detected {type} project. How would you like to configure?"
header: "Setup"
options:
  - label: "Auto-configure (Recommended)"
    description: "Use smart defaults for {type}. Done in one click."
  - label: "Customize"
    description: "Choose stack, focus areas, and review depth."
```

### If Auto-configure -> Skip stack/focus/depth and continue with Step 3d and Step 3e using these defaults:

- **Laravel:** `[rabak-laravel-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **PHP:** `[rabak-laravel-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **Vue/Nuxt:** `[rabak-vue-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **React/Next.js:** `[rabak-typescript-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **Angular:** `[rabak-typescript-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **NestJS:** `[rabak-nest-reviewer, rabak-typescript-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **Java:** `[rabak-java-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **TypeScript:** `[rabak-typescript-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **JavaScript:** `[rabak-typescript-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **Python:** `[rabak-python-reviewer, constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle]`
- **Rust:** `[constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle, architecture-strategist]`
- **Go:** `[constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle, architecture-strategist]`
- **General:** `[constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle, architecture-strategist]`

### If Customize -> Step 3 (all questions)

## Step 3: Configure Details

**a. Stack** -- confirm or override:

```
question: "Which stack should we optimize for?"
header: "Stack"
options:
  - label: "{detected_type} (Recommended)"
    description: "Auto-detected from project files"
  - label: "Laravel"
    description: "Laravel PHP -- adds Laravel convention reviewer"
  - label: "Vue/Nuxt"
    description: "Vue.js / Nuxt -- adds frontend reviewer"
  - label: "React/Next.js"
    description: "React / Next.js -- adds TypeScript reviewer"
  - label: "Angular"
    description: "Angular -- adds TypeScript reviewer"
  - label: "NestJS"
    description: "NestJS -- adds NestJS and TypeScript reviewers"
  - label: "Java"
    description: "Java / JVM -- adds Java reviewer"
  - label: "Python"
    description: "Python -- adds Pythonic pattern reviewer"
  - label: "Rust"
    description: "Rust -- adds architecture reviewer"
  - label: "Go"
    description: "Go -- adds architecture reviewer"
```

Only show options that differ from the detected type.

**b. Focus areas** -- multiSelect:

```
question: "Which review areas matter most?"
header: "Focus"
multiSelect: true
options:
   - label: "Security"
     description: "Vulnerability scanning, auth, input validation (security-sentinel)"
   - label: "Performance"
     description: "N+1 queries, memory leaks, complexity (performance-oracle)"
   - label: "Architecture"
     description: "Design patterns, SOLID, separation of concerns (architecture-strategist)"
   - label: "Code simplicity"
     description: "Over-engineering, DRY failures, readability regressions (code-simplicity-reviewer)"
```

**c. Depth:**

```
question: "How thorough should reviews be?"
header: "Depth"
options:
   - label: "Thorough (Recommended)"
     description: "Stack reviewers + constitution guardrails + all selected focus agents."
   - label: "Fast"
     description: "Stack reviewers + constitution guardrails + code simplicity only."
   - label: "Comprehensive"
     description: "All above + git history, data integrity, and agent-native checks."
```

**d. Delivery loop:**

```
question: "What should the default delivery loop be for this repo?"
header: "TDD"
options:
  - label: "Ralph-driven TDD (Recommended)"
    description: "Failing tests first, minimal implementation second, refactor third, rerun until green and clean. Plans default to unit + e2e evidence unless they record a justified exception."
  - label: "Standard implementation"
    description: "Implementation can lead. Plans may still opt into Ralph explicitly, but any weaker test evidence should be recorded in the plan."
```

**e. Review mode:**

```
question: "When should reviews run during /workflows:work?"
header: "Review mode"
options:
  - label: "Bulk (Default)"
    description: "Runs /workflows-review once at the end of all tasks. This is where named review agents run."
  - label: "Inline"
    description: "Runs template-based inline checks after each task. Does not authorize direct named review-agent dispatch."
  - label: "Both"
    description: "Inline checks per task plus /workflows-review at the end."
```

## Step 4: Build Agent List and Write File

**Stack-specific agents:**
- Laravel/PHP -> `rabak-laravel-reviewer`
- Vue/Nuxt -> `rabak-vue-reviewer`
- React/Next.js/Angular/TypeScript/JavaScript -> `rabak-typescript-reviewer`
- NestJS -> `rabak-nest-reviewer, rabak-typescript-reviewer`
- Java -> `rabak-java-reviewer`
- Python -> `rabak-python-reviewer`
- Rust/Go/General -> (none)

**Always-on baseline agent:**
- Constitution and repo standards -> `constitution-guardian`

**Focus area agents:**
- Security -> `security-sentinel`
- Performance -> `performance-oracle`
- Architecture -> `architecture-strategist`
- Code simplicity -> `code-simplicity-reviewer`

**Depth:**
- Thorough: stack + `constitution-guardian` + selected focus areas
- Fast: stack + `constitution-guardian` + `code-simplicity-reviewer`
- Comprehensive: all above + `git-history-analyzer, data-integrity-guardian, agent-native-reviewer`

**Plan review agents:** stack-specific reviewer + `constitution-guardian` + `code-simplicity-reviewer`.

**Execution settings:**
- `tdd_enabled`: false (default) or true (enables TDD mode in execution agent template)
- `review_mode`: "bulk" (default), "inline", or "both" (`bulk`/`both` invoke `/workflows-review`; `inline` stays template-based and must not spawn named review agents directly)

**Workflow-injected mandatory reviewers:**
- `/workflows-review` always adds `agent-native-reviewer`, `learnings-researcher`, and `uncle-bob`, regardless of `review_agents`.
- `/workflows-architecture` always runs `architecture-strategist` and `uncle-bob`.

Write `compound-engineering.local.md`:

```markdown
---
review_agents: [{computed agent list}]
plan_review_agents: [{computed plan agent list}]
tdd_enabled: {true|false}
tdd:
  precedence: plan_overrides_local
  mode: {ralph|standard}
  loop: {red-green-refactor|implementation-first}
  evidence:
    unit: {required|optional}
    e2e: {required|optional}
  exceptions: []
review_mode: bulk
---

# Review Context

Add project-specific review instructions here.
These notes are passed to `/workflows-review` and to the template-based review steps inside `/workflows-work`. They do not authorize direct named review-agent dispatch outside `/workflows-review`.

## TDD Defaults

- `tdd` is the visible repo-local default contract for planning and execution.
- Plan-level `tdd` values override this file for that plan; `inherit` falls back to these defaults.
- `tdd_enabled` mirrors whether `tdd.mode` is `ralph` until execution templates read the full `tdd` block directly.

Examples:
- "We use Turbo Frames heavily -- check for frame-busting issues"
- "Our API is public -- extra scrutiny on input validation"
- "Performance-critical: we serve 10k req/s on this endpoint"
```

## Step 5: Confirm

```
Saved to compound-engineering.local.md

Stack:        {type}
Review depth: {depth}
Agents:       {count} configured
              {agent list, one per line}
TDD default:  {ralph|standard}
Loop:         {red-green-refactor|implementation-first}
Evidence:     unit={required|optional}, e2e={required|optional}
Precedence:   plan `tdd` values override local defaults when set

Tip: Edit the "Review Context" section to add project-specific instructions.
     Re-run this setup anytime to reconfigure review agents or TDD defaults.
```
