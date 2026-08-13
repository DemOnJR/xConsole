export interface QueuedMessage {
  id: string;
  text: string;
}

export function newQueuedId(): string {
  return (crypto.randomUUID && crypto.randomUUID()) || Math.random().toString(36).slice(2);
}

export function enqueueMessage(list: QueuedMessage[], text: string): QueuedMessage[] {
  const trimmed = text.trim();
  if (!trimmed) return list;
  return [...list, { id: newQueuedId(), text: trimmed }];
}

export function updateQueuedMessage(
  list: QueuedMessage[],
  id: string,
  text: string,
): QueuedMessage[] {
  return list.map((item) => (item.id === id ? { ...item, text } : item));
}

export function removeQueuedMessage(list: QueuedMessage[], id: string): QueuedMessage[] {
  return list.filter((item) => item.id !== id);
}

export function takeNextQueued(list: QueuedMessage[]): {
  next: QueuedMessage | null;
  rest: QueuedMessage[];
} {
  if (list.length === 0) return { next: null, rest: list };
  const [next, ...rest] = list;
  if (!next.text.trim()) return takeNextQueued(rest);
  return { next: { ...next, text: next.text.trim() }, rest };
}
