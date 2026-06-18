import { Toolbar } from "./Toolbar";
import { NewTab } from "../newtab/NewTab";

export function Workspace() {
  return (
    <main class="flex min-w-0 flex-1 flex-col">
      <Toolbar />
      <div class="min-h-0 flex-1 bg-bg">
        <NewTab />
      </div>
    </main>
  );
}
