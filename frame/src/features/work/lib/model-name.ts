import type { WorkRuntimeProjection } from "$shared/ipc/bindings";

/** "gpt-5.6-luna" as a person reads it: "GPT-5.6 Luna". */
export function modelName(id: string): string {
  const [family = "", version, ...rest] = id.split("-");
  const head = /^gpt$/iu.test(family) ? "GPT" : family.charAt(0).toUpperCase() + family.slice(1);
  const words = rest.map((word) => word.charAt(0).toUpperCase() + word.slice(1));
  return [version ? `${head}-${version}` : head, ...words].join(" ");
}

/** The model the latest agent run was granted, if a run has said. */
export function currentModel(projection: WorkRuntimeProjection | null | undefined): string | null {
  for (const execution of [...(projection?.executions ?? [])].reverse())
    for (const node of execution.spec.nodes)
      if (node.capability.kind === "agent") return modelName(node.capability.grant.model);
  return null;
}
