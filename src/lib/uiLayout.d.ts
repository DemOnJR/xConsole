export declare const DRAWER_WIDTH_DEFAULT = 280;
export declare const DRAWER_WIDTH_MIN = 220;
export declare const DRAWER_WIDTH_MAX = 480;
export declare const DRAWER_WIDTH_STEP = 16;
export type DrawerSide = "left" | "right";
export declare function clampDrawerWidth(width: number): number;
export declare function drawerWidthFromDrag(side: DrawerSide, startWidth: number, startX: number, currentX: number): number;
export declare function drawerWidthFromKey(width: number, key: string): number | null;
//# sourceMappingURL=uiLayout.d.ts.map