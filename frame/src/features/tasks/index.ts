/** The task feature's public surface.
 *
 *  A host supplies the session and the chrome; this module supplies the body,
 *  the full destination, and a capture any surface can hand a typed line to. `TaskList` also draws at
 *  `rail` density, so a Work surface composes these rows rather than a second
 *  set that would drift; export its loader here when that host lands.
 */
export const loadTasks = () => import("./components/Tasks.svelte");
export const loadTasksPage = () => import("./components/TasksPage.svelte");
export const loadCapture = () => import("./lib/capture");
export type { TaskScope } from "./lib/task-sections";

export { setPageView as setTaskPageView } from "./lib/page-view.svelte";
