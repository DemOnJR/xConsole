import { describe, expect, it } from "vitest";
import {
  clampDrawerWidth,
  drawerWidthFromDrag,
  drawerWidthFromKey,
  DRAWER_WIDTH_DEFAULT,
  DRAWER_WIDTH_MAX,
  DRAWER_WIDTH_MIN,
  DRAWER_WIDTH_STEP,
} from "./uiLayout";

describe("drawer width constraints", () => {
  it("uses the planned default and clamps both bounds", () => {
    expect(DRAWER_WIDTH_DEFAULT).toBe(280);
    expect(clampDrawerWidth(DRAWER_WIDTH_MIN - 1)).toBe(DRAWER_WIDTH_MIN);
    expect(clampDrawerWidth(DRAWER_WIDTH_MAX + 1)).toBe(DRAWER_WIDTH_MAX);
    expect(clampDrawerWidth(333)).toBe(333);
  });

  it("changes width in the pointer's direction for each drawer side", () => {
    expect(drawerWidthFromDrag("left", 280, 100, 140)).toBe(320);
    expect(drawerWidthFromDrag("right", 280, 100, 140)).toBe(240);
  });
});

describe("drawer keyboard resizing", () => {
  it("moves by the step and clamps at Home and End", () => {
    expect(drawerWidthFromKey(280, "ArrowLeft")).toBe(280 - DRAWER_WIDTH_STEP);
    expect(drawerWidthFromKey(280, "ArrowRight")).toBe(280 + DRAWER_WIDTH_STEP);
    expect(drawerWidthFromKey(280, "Home")).toBe(DRAWER_WIDTH_MIN);
    expect(drawerWidthFromKey(280, "End")).toBe(DRAWER_WIDTH_MAX);
  });

  it("ignores unrelated keys", () => {
    expect(drawerWidthFromKey(280, "Enter")).toBeNull();
  });
});
