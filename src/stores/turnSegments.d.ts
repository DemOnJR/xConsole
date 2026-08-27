import type { StreamEvent } from "../lib/tauri";
import type { AgentActivityItem, AgentChatMessage } from "./agentStore";
/** One chronological slice of an assistant turn: prose or a tool burst. */
export type TurnSegment = {
    type: "text";
    content: string;
} | {
    type: "activity";
    items: AgentActivityItem[];
};
export declare function appendTextDelta(segments: TurnSegment[], delta: string): TurnSegment[];
export declare function applyActivityEvent(segments: TurnSegment[], ev: StreamEvent): TurnSegment[];
export declare function flattenActivity(segments: TurnSegment[]): AgentActivityItem[];
export declare function textFromSegments(segments: TurnSegment[]): string;
/** History without `segments` keeps the old layout (text, then tools). */
export declare function segmentsFromMessage(message: AgentChatMessage): TurnSegment[];
/** Apply a stream event to a flat activity list (one tool-burst). */
export declare function applyStreamEvent(activity: AgentActivityItem[], ev: StreamEvent): AgentActivityItem[];
//# sourceMappingURL=turnSegments.d.ts.map