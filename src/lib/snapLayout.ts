/**
 * Drop targeting for tile windows: swap with the window under the cursor, or
 * dock to one of its edges. No preset shapes — other windows stay put.
 */
export type { DropTarget } from "./tileTree";
export { dropTargetAt, applyDrop } from "./tileTree";
