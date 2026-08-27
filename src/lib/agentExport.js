import { segmentsFromMessage } from "../stores/turnSegments";
const REDACTED = "[REDACTED]";
const SECRET_MARKERS = [
    "password=",
    "password:",
    "passwd=",
    "passwd:",
    "token=",
    "token:",
    "api_key=",
    "api_key:",
    "apikey=",
    "apikey:",
    "secret=",
    "secret:",
    "authorization: bearer ",
    "x-api-key:",
    "--password=",
    "--password ",
    "--token=",
    "--token ",
    "--api-key=",
    "--api-key ",
];
export function redactExportText(input) {
    let output = input.replace(/-----BEGIN [^-]+-----[\s\S]*?-----END [^-]+-----/gi, REDACTED);
    output = output.replace(/([a-z][a-z0-9+.-]*:\/\/[^\s/@]+:)[^\s/@]+@/gi, `$1${REDACTED}@`);
    output = output.replace(/(https?:\/\/[^\s/?#]+[?&](?:token|password|api[_-]?key|secret)=)[^&\s#]+/gi, `$1${REDACTED}`);
    for (const marker of SECRET_MARKERS) {
        output = redactMarker(output, marker);
    }
    return output;
}
function redactMarker(input, marker) {
    const lower = input.toLowerCase();
    const markerLower = marker.toLowerCase();
    let output = "";
    let cursor = 0;
    while (true) {
        const index = lower.indexOf(markerLower, cursor);
        if (index < 0) {
            output += input.slice(cursor);
            return output;
        }
        const valueStart = skipWhitespace(input, index + marker.length);
        if (input.slice(valueStart, valueStart + REDACTED.length) === REDACTED) {
            output += input.slice(cursor, valueStart + REDACTED.length);
            cursor = valueStart + REDACTED.length;
            continue;
        }
        const valueEnd = findValueEnd(input, valueStart);
        output += input.slice(cursor, valueStart) + REDACTED;
        cursor = valueEnd;
    }
}
function skipWhitespace(input, start) {
    let index = start;
    while (/\s/.test(input[index] ?? ""))
        index += 1;
    return index;
}
function findValueEnd(input, start) {
    if (input[start] === '"' || input[start] === "'") {
        const quote = input[start];
        const end = input.indexOf(quote, start + 1);
        return end < 0 ? input.length : end + 1;
    }
    let index = start;
    while (index < input.length && !/[\s;|&,)\]]/.test(input[index]))
        index += 1;
    return index;
}
function exportActivityLine(activity) {
    if (activity.kind === "status" || activity.id === "collapsed-meta")
        return null;
    if (activity.kind === "command")
        return "Ran a command";
    if (activity.kind === "file_edit") {
        const added = activity.linesAdded ?? 0;
        const removed = activity.linesRemoved ?? 0;
        return `Edited a file (+${added}/-${removed} lines)`;
    }
    if (activity.kind === "tool" || activity.kind === "skill_read" || activity.kind === "skill_save") {
        return activity.name ? `Used ${activity.name}` : "Used a tool";
    }
    return activity.kind ? `Activity: ${activity.kind}` : null;
}
export function exportConversationMarkdown(input) {
    const title = redactExportText(input.title?.trim() || "Conversation");
    const lines = [`# ${title}`, ""];
    for (const message of input.messages) {
        if (message.isCompaction) {
            lines.push("---", `*⚡ Context compacted (~${message.compactionTokensBefore ?? "?"} → ~${message.compactionTokensAfter ?? "?"} tokens)*`, "---", "");
            continue;
        }
        if (message.role !== "user" && message.role !== "assistant")
            continue;
        const heading = message.role === "user" ? "User" : "Assistant";
        lines.push(`## ${heading}`, "");
        if (message.role === "user") {
            lines.push(redactExportText(message.content), "");
            if (message.images?.length) {
                lines.push(`*(${message.images.length} image${message.images.length === 1 ? "" : "s"} attached)*`, "");
            }
            continue;
        }
        for (const seg of segmentsFromMessage(message)) {
            if (seg.type === "text") {
                if (seg.content.trim())
                    lines.push(redactExportText(seg.content), "");
                continue;
            }
            const activity = seg.items
                .map(exportActivityLine)
                .filter((line) => line !== null);
            if (activity.length > 0) {
                lines.push("### Activity", ...activity.map((line) => `- ${line}`), "");
            }
        }
    }
    return lines.join("\n").trim() + "\n";
}
//# sourceMappingURL=agentExport.js.map