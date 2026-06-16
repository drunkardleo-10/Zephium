import { For } from "solid-js";
import type { Tab } from "../state/tabs";

interface Props {
  tabs: Tab[];
  activeId: number;
  onSelect: (id: number) => void;
  onClose: (id: number) => void;
  onNew: () => void;
}

export function TabStrip(props: Props) {
  return (
    <div class="tabstrip">
      <For each={props.tabs}>
        {(tab) => (
          <div
            class="tab"
            classList={{ active: tab.id === props.activeId }}
            title={tab.title}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                props.onClose(tab.id);
              } else {
                props.onSelect(tab.id);
              }
            }}
          >
            <span class="tab-spinner" classList={{ active: tab.loading }} />
            <span class="tab-title">{tab.title}</span>
            <button
              class="tab-close"
              aria-label="Close tab"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                props.onClose(tab.id);
              }}
            >
              <svg viewBox="0 0 10 10" width="10" height="10">
                <path d="M1 1 L9 9 M9 1 L1 9" stroke="currentColor" stroke-width="1.4" />
              </svg>
            </button>
          </div>
        )}
      </For>
      <button class="tab-new" aria-label="New tab" onClick={props.onNew}>
        <svg viewBox="0 0 12 12" width="12" height="12">
          <path d="M6 1 V11 M1 6 H11" stroke="currentColor" stroke-width="1.4" />
        </svg>
      </button>
    </div>
  );
}
