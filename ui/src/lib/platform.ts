/** macOS gets an overlay title bar, so the sidebar header leaves room for the traffic lights. */
export const isMac = navigator.userAgent.includes("Mac");

/** Label for the primary modifier key in tooltips. */
export const modKey = isMac ? "⌘" : "Ctrl";
