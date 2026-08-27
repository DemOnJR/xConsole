import type { ChatImage } from "../lib/tauri";
export interface QueuedMessage {
    id: string;
    text: string;
    images?: ChatImage[];
}
export declare function newQueuedId(): string;
export declare function enqueueMessage(list: QueuedMessage[], text: string, images?: ChatImage[]): QueuedMessage[];
export declare function updateQueuedMessage(list: QueuedMessage[], id: string, text: string): QueuedMessage[];
export declare function removeQueuedMessage(list: QueuedMessage[], id: string): QueuedMessage[];
export declare function takeNextQueued(list: QueuedMessage[]): {
    next: QueuedMessage | null;
    rest: QueuedMessage[];
};
//# sourceMappingURL=messageQueue.d.ts.map