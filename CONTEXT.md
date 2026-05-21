# Dynamic Agent Skill Layer

A local-first, self-growing skill context layer that automatically compiles task-relevant skills for coding agent harnesses.

## Language

**Skill**:
A reusable procedural artifact that encodes how-to knowledge for coordinating tools, memory, and runtime context. A Skill has dual existence: (1) a physical SKILL.md file on disk in a scope directory, and (2) a graph node in the multi-level skill graph. Active skills have both a file and a node. Retired or merged skills may exist only as graph nodes. The offline graph-builder service owns keeping files and graph nodes in sync. A skill may be part of a directory with additional reference files (e.g., `references/`, `scripts/`, `assets/`); these supplementary files are stored as reference links on the skill node rather than indexed as independent subunits.
_Avoid_: Tool, API, instruction, command, plugin

**Skill Graph**:
A multi-level graph with three tiers — Skill Communities, Skills, and Subunits — connected by extraction edges (Skill-to-Subunit) and membership edges (Community-to-Skill). Built offline by the graph-builder and queried online by the MCP server.
_Avoid_: Knowledge graph, dependency graph

## Flagged Ambiguities

(None yet)

**Subunit**:
A small, reusable unit extracted from a Skill. Three explicit types:
- **Procedure**: A multi-step workflow or execution instruction (e.g., "Phase 1: Read the work document and extract WHY context")
- **Convention**: A naming rule, pattern constraint, or usage guideline (e.g., "Named review agents belong to /workflows-review")
- **Asset**: A script, configuration snippet, or code block embedded in the skill

Subunits are extracted via hybrid approach: deterministic structural rules for the 80% (headings, code blocks, lists), with Ollama fallback for skills lacking clear structural boundaries. A subunit can belong to multiple skills via extraction edges.
_Avoid_: Chunk, snippet, fragment, instruction block

**Skill Community**:
A group of related skills formed by HDBSCAN clustering over skill embeddings, optionally augmented by manual or automated tags. A community represents a functional or topical domain (e.g., workflows, git, language conventions). Tags defined in skill frontmatter (human-authored or injected by offline skill creation) act as filters at both node level (individual skill membership) and community level (top-level community relations). Skills may belong to multiple communities: their HDBSCAN community plus any tag-based communities. The file scanner watches for tag changes and triggers community re-evaluation.
_Avoid_: Cluster, category, group

**Tag**:
A keyword or short phrase in a skill's frontmatter that signals domain membership. Tags influence community assignment (as filters over HDBSCAN output) and enable filtered retrieval. New tags can be dynamically created by the offline skill creation process if no existing tags match. Tags apply at two levels: node-level (filtering individual skills) and community-level (defining community relations).
_Avoid_: Label, category, topic

**Scope**:
A searchable boundary for skill organization. Two tiers in V1:
- **Project scope**: Bounded by a git repository root. Skills within the repo (in `.skills/`, `.github/skills/`, or other discoverable paths) are project-scoped. One project scope per active repository.
- **Global scope**: An array of machine-wide skill directories, one per supported coding harness (e.g., `~/.config/opencode/skills/`, `~/.claude/skills/`). All harness skill directories are indexed together as the global scope. Configurable via Docker Compose environment variables as a path array.
V2 adds a team scope (remote, shared, online). Retrieval queries multiple scopes concurrently.
_Avoid_: Namespace, partition, domain, context