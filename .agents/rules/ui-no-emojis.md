# UI Iconography & Design Rules

## 1. Strictly NO Emojis
- Never use emojis (e.g. 📁, 📄, 🗄️, ⚡, 🎯, 🧠, 📜, 🐳, 🖥, ✕, ⟳, ⏱, ★, 🔒) as icons, buttons, badges, tree item indicators, or status labels.
- Native emojis render inconsistently across OS platforms and break the professional enterprise aesthetic of xConsole.

## 2. Use Pure SVG Components
- All icons across core and plugins must be clean, crisp SVG components imported from `src/components/icons.tsx` (or the plugin's local `icons.tsx`).
- Icons must have proportional sizing (e.g. `size={12}`, `size={14}`, `size={16}`) and be vertically centered with text (`flex items-center gap-1.5`).

## 3. Aesthetic & Color Palette
- Avoid overly colorful or noisy interfaces.
- Use muted, refined monochrome/neutral palettes (zinc, slate, gray) with subtle, purposeful accent highlights (subtle cyan, amber, violet, emerald) only for active/selected states or critical status alerts.
- Keep the design clean, elegant, high-density, and professional.
