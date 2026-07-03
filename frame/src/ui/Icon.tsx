import { For, type JSX } from "solid-js";

// @hugeicons/core-free-icons ships icons as [tag, react-style attrs] tuples.
export type IconData = readonly (readonly [string, Record<string, string | number>])[];

const ATTR: Record<string, string> = {
  strokeWidth: "stroke-width",
  strokeLinecap: "stroke-linecap",
  strokeLinejoin: "stroke-linejoin",
  fillRule: "fill-rule",
  clipRule: "clip-rule",
};

function attrs(source: Record<string, string | number>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(source)) {
    if (key === "key") continue;
    out[ATTR[key] ?? key] = String(value);
  }
  return out;
}

export function Icon(props: {
  icon: IconData;
  size?: number;
  class?: string;
  style?: JSX.CSSProperties;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={props.size ?? 16}
      height={props.size ?? 16}
      fill="none"
      class={props.class}
      style={props.style}
      aria-hidden="true"
    >
      <For each={props.icon as (readonly [string, Record<string, string | number>])[]}>
        {([tag, source]) => {
          switch (tag) {
            case "path":
              return <path {...attrs(source)} />;
            case "circle":
              return <circle {...attrs(source)} />;
            case "rect":
              return <rect {...attrs(source)} />;
            case "line":
              return <line {...attrs(source)} />;
            case "ellipse":
              return <ellipse {...attrs(source)} />;
            case "polyline":
              return <polyline {...attrs(source)} />;
            case "polygon":
              return <polygon {...attrs(source)} />;
            default:
              return null;
          }
        }}
      </For>
    </svg>
  );
}
