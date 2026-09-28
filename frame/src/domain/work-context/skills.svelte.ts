import { commands } from "$shared/ipc/bindings";
import type {
  WorkSkillChangeV1,
  WorkSkillFaultV1,
  WorkSkillRowV1,
  WorkSkillsResponseV1,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

const TIMEOUT = 9000;

/** The skills the agent follows, built-in and the person's own, as Rust lists them. */
export class WorkSkillsSession {
  skills = $state.raw<WorkSkillRowV1[]>([]);
  loaded = $state(false);
  unavailable = $state(false);
  busy = $state(false);
  /** Why the last change was refused. */
  fault = $state<WorkSkillFaultV1 | null>(null);
  private generation = 0;
  private lifetime = new AbortController();
  constructor(readonly profile: string) {}

  async start() {
    const generation = ++this.generation;
    this.lifetime = new AbortController();
    await this.settle(generation, () => commands.workSkills(this.profile));
  }

  /** A skill's `SKILL.md`; null when it can't be read. */
  async text(name: string): Promise<string | null> {
    const generation = this.generation;
    let text: string | null = null;
    await this.settle(generation, async () => {
      const response = await commands.workSkillText(this.profile, name);
      text = response.text;
      return response;
    });
    return text;
  }

  async change(change: WorkSkillChangeV1) {
    if (this.busy) return false;
    this.busy = true;
    const generation = this.generation;
    const ok = await this.settle(generation, () => commands.workChangeSkill(this.profile, change));
    if (generation === this.generation) this.busy = false;
    return ok && !this.fault;
  }

  private async settle(generation: number, request: () => Promise<WorkSkillsResponseV1>) {
    const response = await observe(Promise.resolve().then(request), TIMEOUT, this.lifetime.signal);
    if (generation !== this.generation) return false;
    if (
      response.state !== "received" ||
      response.value.version !== 1 ||
      response.value.profile !== this.profile ||
      response.value.error
    ) {
      this.unavailable = true;
      return false;
    }
    this.unavailable = false;
    this.loaded = true;
    this.fault = response.value.fault;
    this.skills = response.value.skills;
    return true;
  }

  dispose() {
    this.generation++;
    this.lifetime.abort();
    this.busy = false;
  }
}
