# xConsole Developer & Architecture Rules

## 1. Iconography Rules
- **STRICTLY NO EMOJIS**: Never use emojis anywhere in UI, components, buttons, tabs, tree nodes, or labels.
- **Pure SVG Icons**: All icons across core and plugins must be clean, crisp SVG components imported from `src/components/icons.tsx` (or the plugin's local `icons.tsx`).
- **Muted, Professional Aesthetics**: Avoid noisy or multicolored clutter. Use refined, modern dark mode styling with subtle accents.

## 2. Microkernel & Plugin Architecture
- `xConsole` is a minimal microkernel host.
- Core features (`sftp`, `database`, `agent`, `cloudflare`) live 100% in their standalone repositories under `plugins/` and are pushed to their dedicated GitHub repos (`xconsole-plugin-*`).
- Core communicates with plugins via `@xconsole/sdk` (`src/sdk/index.ts`) and dynamic React Flow nodes (`DynamicPluginNode`).

## 3. Git Workflow Rules
- **First Pull, Then Edit, Then Push**:
  - Always run `git pull` before making any edits in the core repo or any plugin repository (`plugins/*`).
  - Verify changes with appropriate builds and tests (`pnpm run build` / `tsc` / `vitest`).
  - Commit and `git push` all verified changes back to their respective remote repositories.

