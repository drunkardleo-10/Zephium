import type {
  FocusDayView,
  SiteTimeView,
  TimeBucketView,
  TimeCall,
  TimeResponse,
} from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";
import { currentPeriod, reportCall, shiftPeriod, type Period, type Span } from "./time-model";

const RESPONSE_DEADLINE_MS = 8000;
/** Native writes what it counted once a minute, so reading more often than
 *  that would only redraw the same figures. */
const REFRESH_MS = 60_000;

export interface Report {
  buckets: TimeBucketView[];
  previous: TimeBucketView;
  sites: SiteTimeView[];
}

async function call(profile: string, request: TimeCall): Promise<TimeResponse | null> {
  const answer = await observe(commands.timeCall(profile, request), RESPONSE_DEADLINE_MS);
  return answer.state === "received" ? answer.value : null;
}

/** Time for one surface: the period it shows, optionally one site, and that
 *  period's focus. Refreshes while the surface is shown and visible. */
export class TimeSession {
  readonly profile: string;
  period = $state<Period>(currentPeriod("day"));
  site = $state<string | null>(null);
  report = $state.raw<Report | null>(null);
  /** The whole period, kept while one site is in view. */
  overview = $state.raw<Report | null>(null);
  focus = $state.raw<FocusDayView[]>([]);
  loading = $state(false);
  failed = $state(false);

  private request = 0;
  private timer: ReturnType<typeof setInterval> | undefined;
  private onVisible = () => {
    if (document.visibilityState === "visible") void this.refresh();
  };

  constructor(profile: string, span: Span = "day") {
    this.profile = profile;
    this.period = currentPeriod(span);
  }

  /** True while the period is the one happening now. */
  get current() {
    const now = currentPeriod(this.period.span);
    return now.start === this.period.start;
  }

  start() {
    void this.refresh();
    this.timer = setInterval(() => {
      if (document.visibilityState === "visible") void this.refresh();
    }, REFRESH_MS);
    document.addEventListener("visibilitychange", this.onVisible);
  }

  stop() {
    this.request += 1;
    clearInterval(this.timer);
    document.removeEventListener("visibilitychange", this.onVisible);
  }

  show(span: Span) {
    if (span === this.period.span) return;
    this.period = currentPeriod(span);
    this.site = null;
    void this.refresh();
  }

  /** Shows a given period, such as one day picked from a week. */
  view(period: Period) {
    if (period.start > currentPeriod(period.span).start) return;
    this.period = period;
    this.site = null;
    void this.refresh();
  }

  shift(by: number) {
    const next = shiftPeriod(this.period, by);
    if (next.start > currentPeriod(next.span).start) return;
    this.period = next;
    void this.refresh();
  }

  today() {
    this.period = currentPeriod(this.period.span);
    void this.refresh();
  }

  select(site: string | null) {
    if (site === this.site) return;
    this.site = site;
    void this.refresh();
  }

  async refresh() {
    const request = ++this.request;
    const period = this.period;
    const site = this.site;
    this.loading = true;
    const days = period.span === "day" ? 1 : 7;
    const [overview, focused, narrowed] = await Promise.all([
      call(this.profile, reportCall(period)),
      call(this.profile, { kind: "focus_days", from_day: period.start, days }),
      site === null ? Promise.resolve(null) : call(this.profile, reportCall(period, site)),
    ]);
    if (request !== this.request) return;
    this.loading = false;
    if (overview?.kind !== "report") {
      this.failed = true;
      return;
    }
    this.failed = false;
    this.overview = overview;
    this.report = narrowed?.kind === "report" ? narrowed : site === null ? overview : null;
    this.focus = focused?.kind === "focus_days" ? focused.days : [];
  }
}
