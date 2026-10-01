/** Stand-in figures for the day's tiles until their sources exist: the
 *  blocker does not yet keep a count of what it stops, and focus time is not
 *  yet recorded. Each is replaced by its real source as that lands; the page
 *  takes the same numbers either way. */
export const SAMPLE_DAY = {
  trackersBlocked: 1284,
  focusMinutes: 134,
  focusGoalMinutes: 240,
} as const;
