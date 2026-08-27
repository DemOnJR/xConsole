// OS notification helper for when the agent needs the user's attention
// (a command approval, a clarifying question, or a plan to review). Wraps the
// Tauri notification plugin and requests permission lazily on first use.
import { isPermissionGranted, requestPermission, sendNotification, } from "@tauri-apps/plugin-notification";
let permissionState = "unknown";
async function ensurePermission() {
    if (permissionState === "granted")
        return true;
    if (permissionState === "denied")
        return false;
    try {
        let granted = await isPermissionGranted();
        if (!granted) {
            granted = (await requestPermission()) === "granted";
        }
        permissionState = granted ? "granted" : "denied";
        return granted;
    }
    catch {
        permissionState = "denied";
        return false;
    }
}
/** Fire a desktop notification. Best-effort: silently no-ops if denied. */
export async function notify(title, body) {
    try {
        if (!(await ensurePermission()))
            return;
        sendNotification({ title, body });
    }
    catch {
        // Notifications are non-critical — never let them break the flow.
    }
}
//# sourceMappingURL=notify.js.map