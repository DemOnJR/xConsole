export function newQueuedId() {
    return (crypto.randomUUID && crypto.randomUUID()) || Math.random().toString(36).slice(2);
}
export function enqueueMessage(list, text, images) {
    const trimmed = text.trim();
    const pics = images?.length ? images : undefined;
    if (!trimmed && !pics)
        return list;
    return [...list, { id: newQueuedId(), text: trimmed, images: pics }];
}
export function updateQueuedMessage(list, id, text) {
    return list.map((item) => (item.id === id ? { ...item, text } : item));
}
export function removeQueuedMessage(list, id) {
    return list.filter((item) => item.id !== id);
}
export function takeNextQueued(list) {
    if (list.length === 0)
        return { next: null, rest: list };
    const [next, ...rest] = list;
    if (!next.text.trim())
        return takeNextQueued(rest);
    return { next: { ...next, text: next.text.trim() }, rest };
}
//# sourceMappingURL=messageQueue.js.map