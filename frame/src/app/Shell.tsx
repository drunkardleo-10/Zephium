import { Sidebar } from "../features/sidebar/Sidebar";
import { Workspace } from "../features/workspace/Workspace";

export function Shell() {
  return (
    <div class="flex h-screen w-screen overflow-hidden bg-bg text-text">
      <Sidebar />
      <Workspace />
    </div>
  );
}
