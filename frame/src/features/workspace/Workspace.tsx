import { Toolbar } from "./Toolbar";
import { ContentHost } from "./ContentHost";

export function Workspace() {
  return (
    <main class="flex min-w-0 flex-1 flex-col">
      <Toolbar />
      <ContentHost />
    </main>
  );
}
