import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
function base({ size = 16, ...props }) {
    return {
        width: size,
        height: size,
        viewBox: "0 0 24 24",
        fill: "none",
        stroke: "currentColor",
        strokeWidth: 1.8,
        strokeLinecap: "round",
        strokeLinejoin: "round",
        ...props,
    };
}
/** "Toggle left panel" icon: a panel with the left column highlighted. */
export function PanelLeftIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "9", y1: "4", x2: "9", y2: "20" }), _jsx("rect", { x: "3", y: "4", width: "6", height: "16", rx: "2", fill: "currentColor", stroke: "none", opacity: "0.35" })] }));
}
/** "Toggle right panel" icon. */
export function PanelRightIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "15", y1: "4", x2: "15", y2: "20" }), _jsx("rect", { x: "15", y: "4", width: "6", height: "16", rx: "2", fill: "currentColor", stroke: "none", opacity: "0.35" })] }));
}
/** Toggle bottom panel (terminal console). */
export function PanelBottomIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "3", y1: "14", x2: "21", y2: "14" }), _jsx("rect", { x: "3", y: "14", width: "18", height: "6", rx: "2", fill: "currentColor", stroke: "none", opacity: "0.35" })] }));
}
/** Toggle top panel. */
export function PanelTopIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "3", y1: "10", x2: "21", y2: "10" }), _jsx("rect", { x: "3", y: "4", width: "18", height: "6", rx: "2", fill: "currentColor", stroke: "none", opacity: "0.35" })] }));
}
export function MinimizeIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M8 3v3H5M16 3v3h3M8 21v-3H5M16 21v-3h3" }) }));
}
export function SaveIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" }), _jsx("polyline", { points: "17 21 17 13 7 13 7 21" }), _jsx("polyline", { points: "7 3 7 8 15 8" })] }));
}
export function SaveAsIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M16 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9l4 4v6" }), _jsx("polyline", { points: "14 3 14 8 8 8" }), _jsx("line", { x1: "19", y1: "15", x2: "19", y2: "21" }), _jsx("line", { x1: "16", y1: "18", x2: "22", y2: "18" })] }));
}
export function TrashIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("polyline", { points: "3 6 5 6 21 6" }), _jsx("path", { d: "M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" }), _jsx("path", { d: "M10 11v6M14 11v6" }), _jsx("path", { d: "M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" })] }));
}
export function PlusIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("line", { x1: "12", y1: "5", x2: "12", y2: "19" }), _jsx("line", { x1: "5", y1: "12", x2: "19", y2: "12" })] }));
}
/** Database / prompt-cache cylinder. */
export function CacheIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("ellipse", { cx: "12", cy: "6", rx: "7", ry: "2.6" }), _jsx("path", { d: "M5 6v12c0 1.6 3.1 2.8 7 2.8s7-1.2 7-2.8V6" }), _jsx("path", { d: "M5 12c0 1.6 3.1 2.8 7 2.8s7-1.2 7-2.8" })] }));
}
export function EraserIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M20 20H7L3 16a2 2 0 0 1 0-3l9-9a2 2 0 0 1 3 0l5 5a2 2 0 0 1 0 3l-7 7" }), _jsx("line", { x1: "18", y1: "12.5", x2: "11.5", y2: "6" })] }));
}
export function GridIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "3", width: "7", height: "7", rx: "1" }), _jsx("rect", { x: "14", y: "3", width: "7", height: "7", rx: "1" }), _jsx("rect", { x: "3", y: "14", width: "7", height: "7", rx: "1" }), _jsx("rect", { x: "14", y: "14", width: "7", height: "7", rx: "1" })] }));
}
export function MaximizeIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M16 21h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3" }) }));
}
export function TerminalIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("polyline", { points: "7 9 10 12 7 15" }), _jsx("line", { x1: "13", y1: "15", x2: "17", y2: "15" })] }));
}
export function ChevronUpIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polyline", { points: "6 15 12 9 18 15" }) }));
}
export function ChevronDownIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polyline", { points: "6 9 12 15 18 9" }) }));
}
export function ChevronRightIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polyline", { points: "9 6 15 12 9 18" }) }));
}
export function ArrowUpIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("line", { x1: "12", y1: "19", x2: "12", y2: "5" }), _jsx("polyline", { points: "5 12 12 5 19 12" })] }));
}
export function SendIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("line", { x1: "22", y1: "2", x2: "11", y2: "13" }), _jsx("polygon", { points: "22 2 15 22 11 13 2 9 22 2" })] }));
}
export function PlugIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M12 22v-5" }), _jsx("path", { d: "M9 8V2M15 8V2" }), _jsx("path", { d: "M18 8H6v3a6 6 0 0 0 12 0V8z" })] }));
}
export function SettingsIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("circle", { cx: "12", cy: "12", r: "3" }), _jsx("path", { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" })] }));
}
export function FolderIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7l-2-2H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2z" }) }));
}
export function BotIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "4", y: "8", width: "16", height: "12", rx: "2" }), _jsx("path", { d: "M12 8V4M9 4h6" }), _jsx("circle", { cx: "9", cy: "14", r: "1", fill: "currentColor", stroke: "none" }), _jsx("circle", { cx: "15", cy: "14", r: "1", fill: "currentColor", stroke: "none" }), _jsx("path", { d: "M2 13v3M22 13v3" })] }));
}
export function BrainIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M9.5 3a2.5 2.5 0 0 0-2.45 3A2.5 2.5 0 0 0 5 8.5c0 .8.4 1.5 1 2a2.5 2.5 0 0 0 .5 4.5 2.5 2.5 0 0 0 5 0V4.5A1.5 1.5 0 0 0 9.5 3z" }), _jsx("path", { d: "M14.5 3a2.5 2.5 0 0 1 2.45 3A2.5 2.5 0 0 1 19 8.5c0 .8-.4 1.5-1 2a2.5 2.5 0 0 1-.5 4.5 2.5 2.5 0 0 1-5 0" })] }));
}
export function ClockIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("circle", { cx: "12", cy: "12", r: "9" }), _jsx("polyline", { points: "12 7 12 12 16 14" })] }));
}
export function ShieldIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" }) }));
}
export function SparkIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M12 3l1.8 4.7L18.5 9.5 13.8 11.3 12 16l-1.8-4.7L5.5 9.5l4.7-1.8z" }), _jsx("path", { d: "M18 14l.8 2.2L21 17l-2.2.8L18 20l-.8-2.2L15 17l2.2-.8z" })] }));
}
export function MicIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "9", y: "2", width: "6", height: "12", rx: "3" }), _jsx("path", { d: "M5 10v1a7 7 0 0 0 14 0v-1" }), _jsx("line", { x1: "12", y1: "18", x2: "12", y2: "22" }), _jsx("line", { x1: "8", y1: "22", x2: "16", y2: "22" })] }));
}
export function StopIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("rect", { x: "6", y: "6", width: "12", height: "12", rx: "2", fill: "currentColor", stroke: "none" }) }));
}
export function VolumeIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("polygon", { points: "11 5 6 9 2 9 2 15 6 15 11 19 11 5" }), _jsx("path", { d: "M15.5 8.5a5 5 0 0 1 0 7" }), _jsx("path", { d: "M18.5 5.5a9 9 0 0 1 0 13" })] }));
}
export function VolumeOffIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("polygon", { points: "11 5 6 9 2 9 2 15 6 15 11 19 11 5" }), _jsx("line", { x1: "22", y1: "9", x2: "16", y2: "15" }), _jsx("line", { x1: "16", y1: "9", x2: "22", y2: "15" })] }));
}
export function EyeIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" }), _jsx("circle", { cx: "12", cy: "12", r: "3" })] }));
}
export function ImageIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "5", width: "18", height: "14", rx: "2" }), _jsx("circle", { cx: "8.5", cy: "10", r: "1.5" }), _jsx("path", { d: "m21 16-4.5-4.5L9 19" })] }));
}
export function PlanIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "8", y: "2", width: "8", height: "4", rx: "1" }), _jsx("path", { d: "M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-2" }), _jsx("path", { d: "m9 14 2 2 4-4" })] }));
}
export function LockIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "4", y: "11", width: "16", height: "10", rx: "2" }), _jsx("path", { d: "M8 11V7a4 4 0 0 1 8 0v4" })] }));
}
export function LockOpenIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "4", y: "11", width: "16", height: "10", rx: "2" }), _jsx("path", { d: "M8 11V7a4 4 0 0 1 7.5-2" })] }));
}
export function RadarIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "3", width: "18", height: "18", rx: "2" }), _jsx("rect", { x: "13", y: "13", width: "5", height: "5", rx: "1", fill: "currentColor", stroke: "none", opacity: "0.5" })] }));
}
export function ConversationIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M8 9h8M8 13h5" }), _jsx("path", { d: "M4 4h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H8l-4 4V5a1 1 0 0 1 1-1z" })] }));
}
export function LoaderIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M21 12a9 9 0 1 1-6.219-8.56" }) }));
}
export function BookIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M4 19.5A2.5 2.5 0 0 1 6.5 17H20" }), _jsx("path", { d: "M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" })] }));
}
export function PaletteIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("circle", { cx: "13.5", cy: "6.5", r: "1.2", fill: "currentColor", stroke: "none" }), _jsx("circle", { cx: "17.5", cy: "10.5", r: "1.2", fill: "currentColor", stroke: "none" }), _jsx("circle", { cx: "8.5", cy: "7.5", r: "1.2", fill: "currentColor", stroke: "none" }), _jsx("circle", { cx: "6.5", cy: "12.5", r: "1.2", fill: "currentColor", stroke: "none" }), _jsx("path", { d: "M12 2a10 10 0 1 0 0 20c.9 0 1.5-.7 1.5-1.5 0-.4-.2-.8-.4-1-.3-.3-.4-.6-.4-1 0-.8.7-1.5 1.5-1.5H16a6 6 0 0 0 6-6c0-5-4.5-9-10-9z" })] }));
}
/** A database: stacked discs. */
export function DatabaseIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("ellipse", { cx: "12", cy: "6", rx: "8", ry: "3" }), _jsx("path", { d: "M4 6v6c0 1.66 3.58 3 8 3s8-1.34 8-3V6" }), _jsx("path", { d: "M4 12v6c0 1.66 3.58 3 8 3s8-1.34 8-3v-6" })] }));
}
/** Freestyle tiling: an uneven grid — three cells on top, two below. */
export function RowsIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "3", y1: "12", x2: "21", y2: "12" }), _jsx("line", { x1: "9", y1: "4", x2: "9", y2: "12" }), _jsx("line", { x1: "15", y1: "4", x2: "15", y2: "12" }), _jsx("line", { x1: "12", y1: "12", x2: "12", y2: "20" })] }));
}
/** Side-by-side columns: two stacked on the left, one tall on the right. */
export function ColumnsIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "3", y1: "4", x2: "3", y2: "20" }), _jsx("line", { x1: "12", y1: "4", x2: "12", y2: "20" }), _jsx("line", { x1: "3", y1: "12", x2: "12", y2: "12" }), _jsx("line", { x1: "21", y1: "4", x2: "21", y2: "20" })] }));
}
/** A server chassis / targets icon. */
export function ServerIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "2", y: "3", width: "20", height: "7", rx: "2" }), _jsx("rect", { x: "2", y: "14", width: "20", height: "7", rx: "2" }), _jsx("line", { x1: "6", y1: "6.5", x2: "6.01", y2: "6.5", strokeWidth: 2.5 }), _jsx("line", { x1: "6", y1: "17.5", x2: "6.01", y2: "17.5", strokeWidth: 2.5 }), _jsx("line", { x1: "10", y1: "6.5", x2: "18", y2: "6.5", opacity: "0.6" }), _jsx("line", { x1: "10", y1: "17.5", x2: "18", y2: "17.5", opacity: "0.6" })] }));
}
/** An eye-off (crossed out) icon for masked / privacy mode. */
export function EyeOffIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M9.88 9.88a3 3 0 1 0 4.24 4.24" }), _jsx("path", { d: "M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" }), _jsx("path", { d: "M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" }), _jsx("line", { x1: "2", y1: "2", x2: "22", y2: "22" })] }));
}
export function SearchIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("circle", { cx: "11", cy: "11", r: "8" }), _jsx("line", { x1: "21", y1: "21", x2: "16.65", y2: "16.65" })] }));
}
export function FilterIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polygon", { points: "22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" }) }));
}
export function RefreshIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("path", { d: "M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" }) }));
}
export function SlidersIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("line", { x1: "4", y1: "21", x2: "4", y2: "14" }), _jsx("line", { x1: "4", y1: "10", x2: "4", y2: "3" }), _jsx("line", { x1: "12", y1: "21", x2: "12", y2: "12" }), _jsx("line", { x1: "12", y1: "8", x2: "12", y2: "3" }), _jsx("line", { x1: "20", y1: "21", x2: "20", y2: "16" }), _jsx("line", { x1: "20", y1: "12", x2: "20", y2: "3" }), _jsx("line", { x1: "1", y1: "14", x2: "7", y2: "14" }), _jsx("line", { x1: "9", y1: "8", x2: "15", y2: "8" }), _jsx("line", { x1: "17", y1: "16", x2: "23", y2: "16" })] }));
}
export function KeyIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M21 2l-2 2m-1.5 1.5L14 9l-3-3-4 4 3 3-5 5a3.5 3.5 0 0 1-5-5l5-5 3 3 4-4-3-3 3.5-3.5z" }), _jsx("circle", { cx: "7.5", cy: "16.5", r: "1.5" })] }));
}
export function StarFilledIcon(props) {
    return (_jsx("svg", { ...base(props), fill: "currentColor", children: _jsx("polygon", { points: "12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" }) }));
}
export function StarOutlineIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polygon", { points: "12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" }) }));
}
export function CopyIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "9", y: "9", width: "13", height: "13", rx: "2", ry: "2" }), _jsx("path", { d: "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" })] }));
}
export function DownloadIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }), _jsx("polyline", { points: "7 10 12 15 17 10" }), _jsx("line", { x1: "12", y1: "15", x2: "12", y2: "3" })] }));
}
export function UploadIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }), _jsx("polyline", { points: "17 8 12 3 7 8" }), _jsx("line", { x1: "12", y1: "3", x2: "12", y2: "15" })] }));
}
export function PlayIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polygon", { points: "5 3 19 12 5 21 5 3" }) }));
}
export function CheckIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polyline", { points: "20 6 9 17 4 12" }) }));
}
export function CloseIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("line", { x1: "18", y1: "6", x2: "6", y2: "18" }), _jsx("line", { x1: "6", y1: "6", x2: "18", y2: "18" })] }));
}
export function DuplicateIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "8", y: "8", width: "12", height: "12", rx: "2" }), _jsx("path", { d: "M4 16V4a2 2 0 0 1 2-2h12" })] }));
}
export function InfoIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("circle", { cx: "12", cy: "12", r: "10" }), _jsx("line", { x1: "12", y1: "16", x2: "12", y2: "12" }), _jsx("line", { x1: "12", y1: "8", x2: "12.01", y2: "8" })] }));
}
export function SortAscIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M11 5h10M11 9h7M11 13h4" }), _jsx("path", { d: "M3 17l3 3 3-3M6 18V4" })] }));
}
export function SortDescIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("path", { d: "M11 5h10M11 9h7M11 13h4" }), _jsx("path", { d: "M3 7l3-3 3 3M6 6v14" })] }));
}
export function TableIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "3", width: "18", height: "18", rx: "2" }), _jsx("line", { x1: "3", y1: "9", x2: "21", y2: "9" }), _jsx("line", { x1: "3", y1: "15", x2: "21", y2: "15" }), _jsx("line", { x1: "9", y1: "3", x2: "9", y2: "21" })] }));
}
export function ChevronLeftIcon(props) {
    return (_jsx("svg", { ...base(props), children: _jsx("polyline", { points: "15 18 9 12 15 6" }) }));
}
export function PanelLeftCloseIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("rect", { x: "3", y: "4", width: "18", height: "16", rx: "2" }), _jsx("line", { x1: "9", y1: "4", x2: "9", y2: "20" }), _jsx("polyline", { points: "15 10 13 12 15 14" })] }));
}
export function LayersIcon(props) {
    return (_jsxs("svg", { ...base(props), children: [_jsx("polygon", { points: "12 2 2 7 12 12 22 7 12 2" }), _jsx("polyline", { points: "2 17 12 22 22 17" }), _jsx("polyline", { points: "2 12 12 17 22 12" })] }));
}
//# sourceMappingURL=icons.js.map