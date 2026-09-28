const MAC_KEYS: Record<string, string> = {
  CmdOrCtrl: "⌘",
  Cmd: "⌘",
  Command: "⌘",
  Super: "⌘",
  Ctrl: "⌃",
  Control: "⌃",
  Shift: "⇧",
  Alt: "⌥",
  Option: "⌥",
};
const OTHER_KEYS: Record<string, string> = {
  CmdOrCtrl: "Ctrl",
  Command: "Ctrl",
  Cmd: "Ctrl",
  Control: "Ctrl",
  Option: "Alt",
  Super: "Win",
};
const NAMED: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "↵",
  Escape: "Esc",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

/** An accelerator such as `CmdOrCtrl+Shift+T` or `Alt+KeyK` as the keycaps a
 *  person presses. Every modifier is translated, and key codes read as the
 *  key's own label. */
export function acceleratorKeys(accelerator: string, mac: boolean): string[] {
  if (!accelerator.trim()) return [];
  return accelerator
    .split("+")
    .filter(Boolean)
    .map((key) => {
      const modifier = (mac ? MAC_KEYS : OTHER_KEYS)[key];
      if (modifier) return modifier;
      const code = /^(?:Key|Digit)(.)$/u.exec(key);
      return code ? code[1]! : (NAMED[key] ?? key);
    });
}

const MODIFIER_CODES = new Set([
  "MetaLeft",
  "MetaRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "CapsLock",
  "Fn",
]);

/** The accelerator a key press spells, by physical key so it does not change
 *  with the keyboard layout, or null while only modifiers are held. */
export function acceleratorFrom(event: KeyboardEvent, mac: boolean): string | null {
  if (!event.code || MODIFIER_CODES.has(event.code)) return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push(mac ? "Cmd" : "Super");
  parts.push(event.code);
  return parts.join("+");
}
