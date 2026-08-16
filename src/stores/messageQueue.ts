import type { ChatImage } from "../lib/tauri";

export interface QueuedMessage {
  id: string;
  text: string;
  images?: ChatImage[];
}

export function newQueuedId(): string {
  return (crypto.randomUUID && crypto.randomUUID()) || Math.random().toString(36).slice(2);
}

export function enqueueMessage(
  list: QueuedMessage[],
  text: string,
  images?: ChatImage[],
): QueuedMessage[] {
  const trimmed = text.trim();
  const pics = images?.length ? images : undefined;
  if (!trimmed && !pics) return list;
  return [...list, { id: newQueuedId(), text: trimmed, images: pics }];
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
