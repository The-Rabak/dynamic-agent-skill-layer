---
name: frontend-design
description: This skill should be used when creating distinctive, production-grade frontend interfaces with high design quality. It applies when the user asks to build web components, pages, or applications. Generates creative, polished code that avoids generic AI aesthetics.
model: gpt-5.3-codex
---

This skill guides creation of distinctive, production-grade frontend interfaces that avoid generic "AI slop" aesthetics. Implement real working code with exceptional attention to aesthetic details and creative choices.

The user provides frontend requirements: a component, page, application, or interface to build. They may include context about the purpose, audience, or technical constraints.

## Design Thinking

Before coding, understand the context and commit to a BOLD aesthetic direction:
- **Purpose**: What problem does this interface solve? Who uses it?
- **Tone**: Pick an extreme: brutally minimal, maximalist chaos, retro-futuristic, organic/natural, luxury/refined, playful/toy-like, editorial/magazine, brutalist/raw, art deco/geometric, soft/pastel, industrial/utilitarian, etc. There are so many flavors to choose from. Use these for inspiration but design one that is true to the aesthetic direction.
- **Constraints**: Technical requirements (framework, performance, accessibility).
- **Differentiation**: What makes this UNFORGETTABLE? What's the one thing someone will remember?

**CRITICAL**: Choose a clear conceptual direction and execute it with precision. Bold maximalism and refined minimalism both work -- the key is intentionality, not intensity.

## Framework-Specific Patterns

Adapt to the project's framework. Detect and follow existing conventions:

**React / Next.js:**
- Functional components with hooks
- CSS Modules, Tailwind CSS, or styled-components (match project convention)
- Use `motion` (Framer Motion) for animations when available
- Component composition over inheritance

**Vue 3 / Nuxt 3:**
- Composition API with `<script setup>`
- Scoped styles with `<style scoped>`
- Composables for shared logic
- `defineProps` / `defineEmits` for component contracts

**Angular:**
- Standalone components (Angular 17+)
- Signal-based reactivity where supported
- OnPush change detection strategy
- Angular CDK for accessible primitives

**Svelte / SvelteKit:**
- Reactive declarations with `$:`
- Svelte transitions and animations
- Component slots for composition

**Vanilla / Framework-agnostic:**
- Web Components with Shadow DOM when encapsulation matters
- CSS custom properties for theming
- Progressive enhancement

### General Conventions (all frameworks)

- Use `rem` for sizing, `px` only for borders and fine details
- Use CSS custom properties (`--var`) for colors, spacing, and theming
- Mobile-first responsive design
- Semantic HTML elements (`<nav>`, `<main>`, `<article>`, `<section>`)
- Follow the project's existing naming convention for CSS classes

Then implement working code that is:
- Production-grade and functional
- Visually striking and memorable
- Cohesive with a clear aesthetic point-of-view
- Meticulously refined in every detail

## Component Design Principles

- **Single Responsibility**: Each component does one thing well
- **Composition over Configuration**: Prefer composable primitives over mega-components with dozens of props
- **Controlled vs Uncontrolled**: Be explicit about state ownership
- **Prop Drilling Prevention**: Use context/provide-inject/stores for deeply shared state
- **Accessible by Default**: All interactive elements must be keyboard-navigable and screen-reader friendly (ARIA attributes, focus management, color contrast)

## CSS Architecture

- **Design Tokens**: Define spacing, color, typography, and shadow scales as CSS custom properties or design token files
- **Responsive Strategy**: Mobile-first with `min-width` breakpoints. Common breakpoints: 640px, 768px, 1024px, 1280px
- **Dark Mode**: Support `prefers-color-scheme` and manual toggle via CSS custom properties or class-based theming
- **Layout**: Use CSS Grid for page-level layouts, Flexbox for component-level alignment
- **Spacing Scale**: Consistent spacing (e.g., 4px base: 4, 8, 12, 16, 24, 32, 48, 64)

## Accessibility Requirements

- Minimum contrast ratio: 4.5:1 for normal text, 3:1 for large text (WCAG AA)
- All images require meaningful `alt` text (or `alt=""` for decorative)
- Focus indicators must be visible and styled intentionally
- Interactive elements must have accessible names
- Forms need associated labels, error messages linked via `aria-describedby`
- Motion: respect `prefers-reduced-motion` -- disable or reduce animations

## Frontend Aesthetics Guidelines

Focus on:
- **Typography**: Choose fonts that are beautiful, unique, and interesting. Avoid generic fonts like Arial and Inter; opt instead for distinctive choices that elevate the frontend's aesthetics; unexpected, characterful font choices. Pair a distinctive display font with a refined body font.
- **Color & Theme**: Commit to a cohesive aesthetic. Use CSS variables for consistency. Dominant colors with sharp accents outperform timid, evenly-distributed palettes.
- **Motion**: Use animations for effects and micro-interactions. Prioritize CSS-only solutions for HTML. Use Motion library for React when available. Focus on high-impact moments: one well-orchestrated page load with staggered reveals (animation-delay) creates more delight than scattered micro-interactions. Use scroll-triggering and hover states that surprise.
- **Spatial Composition**: Unexpected layouts. Asymmetry. Overlap. Diagonal flow. Grid-breaking elements. Generous negative space OR controlled density.
- **Backgrounds & Visual Details**: Create atmosphere and depth rather than defaulting to solid colors. Add contextual effects and textures that match the overall aesthetic. Apply creative forms like gradient meshes, noise textures, geometric patterns, layered transparencies, dramatic shadows, decorative borders, custom cursors, and grain overlays.

NEVER use generic AI-generated aesthetics like overused font families (Inter, Roboto, Arial, system fonts), cliched color schemes (particularly purple gradients on white backgrounds), predictable layouts and component patterns, and cookie-cutter design that lacks context-specific character.

Interpret creatively and make unexpected choices that feel genuinely designed for the context. No design should be the same. Vary between light and dark themes, different fonts, different aesthetics. NEVER converge on common choices (Space Grotesk, for example) across generations.

**IMPORTANT**: Match implementation complexity to the aesthetic vision. Maximalist designs need elaborate code with extensive animations and effects. Minimalist or refined designs need restraint, precision, and careful attention to spacing, typography, and subtle details. Elegance comes from executing the vision well.

Remember: Claude is capable of extraordinary creative work. Don't hold back, show what can truly be created when thinking outside the box and committing fully to a distinctive vision.
