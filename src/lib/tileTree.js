/**
 * Recursive split-tree tiling.
 *
 * Rows-only or columns-only cannot express "3 on top, 1 in the middle, 4 on the
 * bottom, and one full-height pane on the right". A tree can: a row of
 * [column of those bands, right leaf].
 *
 * Drag operations (swap / dock) edit two leaves and leave every other window
 * where it is. Inferring the tree from positions is how Tile becomes automatic.
 */
const MIN_WEIGHT = 0.25;
const MAX_WEIGHT = 8;
const MIN_FILL = 0.2;
const clampW = (w) => Math.min(MAX_WEIGHT, Math.max(MIN_WEIGHT, Number.isFinite(w) ? w : 1));
export function leaf(id, weight = 1) {
    return { kind: "leaf", id, weight: clampW(weight) };
}
export function treeIds(node) {
    if (node.kind === "leaf")
        return [node.id];
    return node.kids.flatMap(treeIds);
}
export function cloneSplit(node) {
    if (node.kind === "leaf")
        return { ...node };
    return { kind: node.kind, weight: node.weight, kids: node.kids.map(cloneSplit) };
}
/** Collapse single-child splits and drop empty ones. */
export function prune(node) {
    if (!node)
        return null;
    if (node.kind === "leaf")
        return node;
    const kids = node.kids.map(prune).filter((k) => k !== null);
    if (kids.length === 0)
        return null;
    if (kids.length === 1)
        return { ...kids[0], weight: node.weight };
    return { kind: node.kind, weight: clampW(node.weight), kids };
}
export function treeFromIdsRow(ids) {
    if (ids.length === 0)
        return { kind: "row", weight: 1, kids: [] };
    if (ids.length === 1)
        return leaf(ids[0]);
    return { kind: "row", weight: 1, kids: ids.map((id) => leaf(id)) };
}
/** Balanced default as a column of rows (same shape as autoLayout). */
export function autoTree(ids) {
    if (ids.length === 0)
        return { kind: "col", weight: 1, kids: [] };
    if (ids.length === 1)
        return leaf(ids[0]);
    const cols = Math.ceil(Math.sqrt(ids.length));
    const rows = Math.ceil(ids.length / cols);
    const base = Math.floor(ids.length / rows);
    const extra = ids.length % rows;
    const kids = [];
    let i = 0;
    for (let r = 0; r < rows; r++) {
        const n = base + (r < extra ? 1 : 0);
        const slice = ids.slice(i, i + n);
        i += n;
        kids.push(slice.length === 1 ? leaf(slice[0]) : treeFromIdsRow(slice));
    }
    return kids.length === 1 ? kids[0] : { kind: "col", weight: 1, kids };
}
function groupsOnAxis(nodes, axis) {
    const start = (n) => (axis === "x" ? n.x : n.y);
    const end = (n) => (axis === "x" ? n.x + n.width : n.y + n.height);
    const sorted = [...nodes].sort((a, b) => start(a) - start(b) || start(a) - start(b));
    const groups = [];
    let cur = [];
    let curEnd = -Infinity;
    for (const n of sorted) {
        if (cur.length > 0 && start(n) < curEnd - 2) {
            cur.push(n);
            curEnd = Math.max(curEnd, end(n));
        }
        else {
            if (cur.length)
                groups.push(cur);
            cur = [n];
            curEnd = end(n);
        }
    }
    if (cur.length)
        groups.push(cur);
    return groups;
}
/**
 * Read a split tree from where the windows sit.
 *
 * Vertical gaps (a tall pane on the right) are preferred, then horizontal bands.
 * Each group is solved the same way, so "3 / 1 / 4 + one on the right" falls out
 * without a shape picker.
 */
export function treeFromPositions(nodes) {
    if (nodes.length === 0)
        return { kind: "col", weight: 1, kids: [] };
    if (nodes.length === 1)
        return leaf(nodes[0].id);
    const xGroups = groupsOnAxis(nodes, "x");
    if (xGroups.length >= 2) {
        const totalW = xGroups.reduce((s, g) => s + Math.max(...g.map((n) => n.x + n.width)) - Math.min(...g.map((n) => n.x)), 0);
        return {
            kind: "row",
            weight: 1,
            kids: xGroups.map((g) => {
                const span = Math.max(...g.map((n) => n.x + n.width)) - Math.min(...g.map((n) => n.x));
                const child = treeFromPositions(g);
                return { ...child, weight: clampW(totalW > 0 ? span / (totalW / xGroups.length) : 1) };
            }),
        };
    }
    const yGroups = groupsOnAxis(nodes, "y");
    if (yGroups.length >= 2) {
        const totalH = yGroups.reduce((s, g) => s + Math.max(...g.map((n) => n.y + n.height)) - Math.min(...g.map((n) => n.y)), 0);
        return {
            kind: "col",
            weight: 1,
            kids: yGroups.map((g) => {
                const span = Math.max(...g.map((n) => n.y + n.height)) - Math.min(...g.map((n) => n.y));
                const child = treeFromPositions(g);
                return { ...child, weight: clampW(totalH > 0 ? span / (totalH / yGroups.length) : 1) };
            }),
        };
    }
    // Overlapping pile: keep left-to-right order as a row.
    const ordered = [...nodes].sort((a, b) => a.x + a.width / 2 - (b.x + b.width / 2) || a.y - b.y);
    return treeFromIdsRow(ordered.map((n) => n.id));
}
export function computeTreeBoxes(node, x, y, width, height) {
    if (width <= 0 || height <= 0)
        return [];
    if (node.kind === "leaf") {
        return [{ id: node.id, x, y, width, height }];
    }
    const kids = node.kids;
    if (kids.length === 0)
        return [];
    const total = kids.reduce((s, k) => s + clampW(k.weight), 0);
    const boxes = [];
    if (node.kind === "row") {
        let cx = x;
        kids.forEach((kid, i) => {
            const last = i === kids.length - 1;
            const w = last
                ? x + width - cx
                : Math.max(1, Math.floor((width * clampW(kid.weight)) / total));
            boxes.push(...computeTreeBoxes(kid, cx, y, w, height));
            cx += w;
        });
    }
    else {
        let cy = y;
        kids.forEach((kid, i) => {
            const last = i === kids.length - 1;
            const h = last
                ? y + height - cy
                : Math.max(1, Math.floor((height * clampW(kid.weight)) / total));
            boxes.push(...computeTreeBoxes(kid, x, cy, width, h));
            cy += h;
        });
    }
    return boxes;
}
export function layoutFromTree(tree, fillW, fillH) {
    const ids = treeIds(tree);
    const rows = [{ weight: 1, items: ids.map((id) => ({ id, weight: 1 })) }];
    return { rows, tree, fillW, fillH };
}
export function treeOf(layout) {
    if (layout.tree)
        return layout.tree;
    if (layout.columns && layout.columns.length > 0) {
        return {
            kind: "row",
            weight: 1,
            kids: layout.columns.map((c) => c.items.length === 1
                ? { ...leaf(c.items[0].id, c.items[0].weight), weight: clampW(c.weight) }
                : {
                    kind: "col",
                    weight: clampW(c.weight),
                    kids: c.items.map((it) => leaf(it.id, it.weight)),
                }),
        };
    }
    if (layout.rows.length === 0)
        return { kind: "col", weight: 1, kids: [] };
    if (layout.rows.length === 1 && layout.rows[0].items.length === 1) {
        return leaf(layout.rows[0].items[0].id, layout.rows[0].items[0].weight);
    }
    return {
        kind: "col",
        weight: 1,
        kids: layout.rows.map((r) => r.items.length === 1
            ? { ...leaf(r.items[0].id, r.items[0].weight), weight: clampW(r.weight) }
            : {
                kind: "row",
                weight: clampW(r.weight),
                kids: r.items.map((it) => leaf(it.id, it.weight)),
            }),
    };
}
function mapLeaves(node, fn) {
    if (node.kind === "leaf")
        return { ...node, id: fn(node.id) };
    return { ...node, kids: node.kids.map((k) => mapLeaves(k, fn)) };
}
/** Swap two windows. Every other tile stays put. */
export function swapLeaves(layout, a, b) {
    if (a === b)
        return layout;
    const tree = prune(mapLeaves(treeOf(layout), (id) => (id === a ? b : id === b ? a : id)));
    return tree ? layoutFromTree(tree, layout.fillW, layout.fillH) : layout;
}
function removeLeaf(node, id) {
    if (node.kind === "leaf")
        return node.id === id ? null : node;
    const kids = node.kids.map((k) => removeLeaf(k, id)).filter((k) => k !== null);
    if (kids.length === 0)
        return null;
    if (kids.length === 1)
        return { ...kids[0], weight: node.weight };
    return { kind: node.kind, weight: node.weight, kids };
}
function replaceLeaf(node, id, next) {
    if (node.kind === "leaf")
        return node.id === id ? { ...next, weight: node.weight } : node;
    return { ...node, kids: node.kids.map((k) => replaceLeaf(k, id, next)) };
}
/** Dock `dragged` onto an edge of `target`. Other windows keep their places. */
export function dockLeaf(layout, dragged, target, edge) {
    if (dragged === target)
        return layout;
    let tree = treeOf(layout);
    tree = removeLeaf(tree, dragged) ?? tree;
    const pairKind = edge === "left" || edge === "right" ? "row" : "col";
    const first = edge === "left" || edge === "top";
    const moved = leaf(dragged);
    const host = leaf(target);
    const pair = {
        kind: pairKind,
        weight: 1,
        kids: first ? [moved, host] : [host, moved],
    };
    tree = replaceLeaf(tree, target, pair);
    const pruned = prune(tree);
    return pruned ? layoutFromTree(pruned, layout.fillW, layout.fillH) : layout;
}
/** Dock against the outer pane (new split at the root). */
export function dockToPane(layout, dragged, edge) {
    let tree = removeLeaf(treeOf(layout), dragged);
    const moved = leaf(dragged);
    if (!tree)
        return layoutFromTree(moved, layout.fillW, layout.fillH);
    const pairKind = edge === "left" || edge === "right" ? "row" : "col";
    const first = edge === "left" || edge === "top";
    const root = {
        kind: pairKind,
        weight: 1,
        kids: first ? [moved, tree] : [tree, moved],
    };
    const pruned = prune(root);
    return pruned ? layoutFromTree(pruned, layout.fillW, layout.fillH) : layout;
}
export function reconcileTree(tree, ids) {
    if (!tree)
        return autoTree(ids);
    const live = new Set(ids);
    const stripped = prune((function strip(n) {
        if (n.kind === "leaf")
            return live.has(n.id) ? n : null;
        const kids = n.kids.map(strip).filter((k) => k !== null);
        if (kids.length === 0)
            return null;
        if (kids.length === 1)
            return { ...kids[0], weight: n.weight };
        return { kind: n.kind, weight: n.weight, kids };
    })(tree));
    const base = stripped ?? autoTree(ids);
    const placed = new Set(treeIds(base));
    const extras = ids.filter((id) => !placed.has(id));
    if (extras.length === 0)
        return base;
    return (prune({
        kind: "row",
        weight: 1,
        kids: [base, ...extras.map((id) => leaf(id))],
    }) ?? autoTree(ids));
}
/** Trade size with the sibling on the matching axis. */
export function resizeTree(layout, id, dw, dh) {
    const tree = cloneSplit(treeOf(layout));
    function walk(node, _axis) {
        if (node.kind === "leaf")
            return node.id === id;
        const idx = node.kids.findIndex((k) => walk(k, node.kind));
        if (idx < 0)
            return false;
        const delta = node.kind === "row" ? dw : dh;
        if (Math.abs(delta) < 1e-6)
            return true;
        if (node.kids.length < 2)
            return true;
        const neighbour = idx + 1 < node.kids.length ? idx + 1 : idx - 1;
        const a = node.kids[idx];
        const b = node.kids[neighbour];
        const before = a.weight;
        a.weight = clampW(a.weight + delta);
        const applied = a.weight - before;
        const afterB = clampW(b.weight - applied);
        const absorbed = b.weight - afterB;
        a.weight = before + absorbed;
        b.weight = afterB;
        return true;
    }
    walk(tree, null);
    return layoutFromTree(tree, layout.fillW, layout.fillH);
}
export function moveInTree(layout, id, dir, axis) {
    const want = axis === "horizontal" ? "row" : "col";
    const tree = cloneSplit(treeOf(layout));
    function walk(node) {
        if (node.kind === "leaf")
            return node.id === id;
        const idx = node.kids.findIndex(walk);
        if (idx < 0)
            return false;
        if (node.kind === want) {
            const to = idx + dir;
            if (to < 0 || to >= node.kids.length)
                return true;
            [node.kids[idx], node.kids[to]] = [node.kids[to], node.kids[idx]];
            return true;
        }
        return true;
    }
    walk(tree);
    return layoutFromTree(tree, layout.fillW, layout.fillH);
}
const EDGE = 0.22;
const PANE_EDGE = 0.08;
function edgeOfBox(x, y, box) {
    if (x < box.x ||
        y < box.y ||
        x > box.x + box.width ||
        y > box.y + box.height) {
        return null;
    }
    const rx = (x - box.x) / Math.max(box.width, 1);
    const ry = (y - box.y) / Math.max(box.height, 1);
    const dl = rx;
    const dr = 1 - rx;
    const dt = ry;
    const db = 1 - ry;
    const m = Math.min(dl, dr, dt, db);
    if (m > EDGE)
        return "center";
    if (m === dl)
        return "left";
    if (m === dr)
        return "right";
    if (m === dt)
        return "top";
    return "bottom";
}
export function dropTargetAt(boxes, x, y, paneW, paneH, draggedId) {
    if (paneW <= 0 || paneH <= 0)
        return null;
    const paneLeft = x <= paneW * PANE_EDGE;
    const paneRight = x >= paneW * (1 - PANE_EDGE);
    const paneTop = y <= paneH * PANE_EDGE;
    const paneBottom = y >= paneH * (1 - PANE_EDGE);
    for (const box of boxes) {
        if (box.id === draggedId)
            continue;
        const hit = edgeOfBox(x, y, box);
        if (!hit)
            continue;
        if (hit === "center") {
            return { kind: "swap", targetId: box.id, x: box.x, y: box.y, width: box.width, height: box.height };
        }
        const halfW = box.width / 2;
        const halfH = box.height / 2;
        const rect = hit === "left"
            ? { x: box.x, y: box.y, width: halfW, height: box.height }
            : hit === "right"
                ? { x: box.x + halfW, y: box.y, width: halfW, height: box.height }
                : hit === "top"
                    ? { x: box.x, y: box.y, width: box.width, height: halfH }
                    : { x: box.x, y: box.y + halfH, width: box.width, height: halfH };
        return { kind: "dock", targetId: box.id, edge: hit, ...rect };
    }
    if (paneLeft)
        return { kind: "pane", edge: "left", x: 0, y: 0, width: paneW * 0.35, height: paneH };
    if (paneRight)
        return { kind: "pane", edge: "right", x: paneW * 0.65, y: 0, width: paneW * 0.35, height: paneH };
    if (paneTop)
        return { kind: "pane", edge: "top", x: 0, y: 0, width: paneW, height: paneH * 0.3 };
    if (paneBottom)
        return { kind: "pane", edge: "bottom", x: 0, y: paneH * 0.7, width: paneW, height: paneH * 0.3 };
    return null;
}
export function applyDrop(layout, draggedId, drop) {
    if (drop.kind === "swap" && drop.targetId)
        return swapLeaves(layout, draggedId, drop.targetId);
    if (drop.kind === "dock" && drop.targetId && drop.edge) {
        return dockLeaf(layout, draggedId, drop.targetId, drop.edge);
    }
    if (drop.kind === "pane" && drop.edge)
        return dockToPane(layout, draggedId, drop.edge);
    return layout;
}
export function fillOfTree(layout) {
    return {
        w: Math.min(1, Math.max(MIN_FILL, layout.fillW ?? 1)),
        h: Math.min(1, Math.max(MIN_FILL, layout.fillH ?? 1)),
    };
}
export function serializeSplit(node, indexOf) {
    if (node.kind === "leaf") {
        const index = indexOf(node.id);
        if (index < 0)
            return null;
        return { kind: "leaf", index, weight: node.weight };
    }
    const kids = node.kids
        .map((k) => serializeSplit(k, indexOf))
        .filter((k) => k !== null);
    if (kids.length === 0)
        return null;
    return { kind: node.kind, weight: node.weight, kids };
}
export function deserializeSplit(saved, ids) {
    if (saved.kind === "leaf") {
        const id = ids[saved.index];
        if (!id)
            return null;
        return leaf(id, saved.weight);
    }
    if (!Array.isArray(saved.kids))
        return null;
    const kids = saved.kids
        .map((k) => deserializeSplit(k, ids))
        .filter((k) => k !== null);
    if (kids.length === 0)
        return null;
    return { kind: saved.kind, weight: saved.weight ?? 1, kids };
}
//# sourceMappingURL=tileTree.js.map