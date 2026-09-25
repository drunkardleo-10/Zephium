/**
 * Host traits the chrome must branch on. Kept in one place so a component
 * never re-derives the platform from the user agent.
 *
 * macOS keeps AppKit's traffic lights through an overlay title bar; the other
 * platforms run undecorated and draw their own window controls in the sidebar.
 */
export const IS_MAC = navigator.userAgent.includes("Mac");
export const IS_WINDOWS = navigator.userAgent.includes("Windows");
