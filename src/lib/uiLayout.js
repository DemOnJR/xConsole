export const DRAWER_WIDTH_DEFAULT = 280;
export const DRAWER_WIDTH_MIN = 220;
export const DRAWER_WIDTH_MAX = 480;
export const DRAWER_WIDTH_STEP = 16;
export function clampDrawerWidth(width) {
    return Math.round(Math.min(DRAWER_WIDTH_MAX, Math.max(DRAWER_WIDTH_MIN, width)));
}
export function drawerWidthFromDrag(side, startWidth, startX, currentX) {
    const direction = side === "left" ? 1 : -1;
    return clampDrawerWidth(startWidth + (currentX - startX) * direction);
}
export function drawerWidthFromKey(width, key) {
    if (key === "Home")
        return DRAWER_WIDTH_MIN;
    if (key === "End")
        return DRAWER_WIDTH_MAX;
    if (key === "ArrowLeft")
        return clampDrawerWidth(width - DRAWER_WIDTH_STEP);
    if (key === "ArrowRight")
        return clampDrawerWidth(width + DRAWER_WIDTH_STEP);
    return null;
}
//# sourceMappingURL=uiLayout.js.map