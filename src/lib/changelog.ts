/**
 * App-level changelog that drives the onboarding wizard's "What's New" step.
 *
 * Versioning contract:
 *   - `CURRENT_ONBOARDING_VERSION` is bumped whenever a release introduces a
 *     change that the user materially needs to see (data-flow rewrite, new
 *     permission, new install step). Cosmetic / patch releases do NOT bump
 *     this — keep it conservative or every minor patch re-onboards users.
 *   - On launch, `loadSettings` compares `lastOnboardedVersion` against
 *     this constant. If they differ, the wizard re-opens with a "What's
 *     New" step rendering every entry whose `version` is newer than the
 *     stored stamp.
 *
 * Data shape:
 *   - Each entry is a list of `highlights` — one bullet per change,
 *     with a short title (3-4 words) and a punchy one-line body.
 *     Frame each around what the user *gets*, not what we changed
 *     under the hood.
 *   - 2-5 highlights per release reads well; more than that and the
 *     wizard step feels like a release-notes dump rather than a
 *     summary.
 *
 * Length budget (the rule that keeps the changelog readable):
 *   - The popover frame is 340px wide and the changelog scroll area
 *     caps at 280px. The whole entry must fit in one viewport so the
 *     user reads it at a glance without scrolling. Concretely:
 *       - title: ≤ 30 chars (one line at 11px)
 *       - description: ≤ 60 chars (one line at 10.5px) — two lines
 *         only if you genuinely cannot say it shorter
 *   - If you find yourself reaching for an em-dash or compound
 *     clause, split into two highlights or cut the second clause.
 *
 * Design constraints:
 *   - One source of truth: future "About" / "Release notes" panels read
 *     this same array. Do not duplicate copy in onboarding components.
 *   - Highlights are user-visible — frame them around what the user does
 *     differently, not what the implementation looks like.
 *   - Order is newest-first. The wizard renders top-down so the most
 *     relevant change leads.
 */

export interface ChangelogHighlight {
  /** Two-to-four-word headline that fits on a single line of a hero card. */
  title: string;
  /** One-sentence body. Speak to the user's experience, not the code path. */
  description: string;
}

export interface ChangelogEntry {
  /** Semver string the entry shipped under, e.g. "0.12.0". */
  version: string;
  /** ISO yyyy-mm-dd release date. */
  date: string;
  /** One-line headline for the version. */
  title: string;
  /**
   * User-facing summaries rendered as cards. Aim for 2-5 per release —
   * each card is a clean section title + one-sentence body.
   */
  highlights: ChangelogHighlight[];
  /** Optional badge for the version row (e.g. "Major rewrite"). */
  tag?: string;
}

/**
 * The version the **current** wizard flow corresponds to. When `loadSettings`
 * sees a saved `lastOnboardedVersion` that doesn't match this, the user is
 * re-onboarded so they see the changelog and re-run any new install steps.
 */
export const CURRENT_ONBOARDING_VERSION = "0.12.0";

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "0.12.0",
    date: "2026-04-29",
    title: "Statusline-based rate limits",
    tag: "Major rewrite",
    highlights: [
      {
        title: "No more Keychain prompts",
        description: "Live limits come from Claude Code directly — no OAuth.",
      },
      {
        title: "Cross-platform parity",
        description: "Windows and Linux now match macOS.",
      },
      {
        title: "Accurate, consistent readings",
        description: "Percentages always match what Claude Code reports.",
      },
      {
        title: "Refined Settings & onboarding",
        description: "Cleaner copy, calmer status indicators.",
      },
    ],
  },
];

/**
 * Compare two semver strings. Returns negative when a < b, zero when equal,
 * positive when a > b. Tolerates missing patch components ("0.11" vs
 * "0.11.0") and ignores any pre-release suffix after a `-`.
 */
export function compareSemver(a: string, b: string): number {
  const parse = (v: string): number[] =>
    v
      .split("-")[0]
      .split(".")
      .map((segment) => Number.parseInt(segment, 10) || 0);

  const partsA = parse(a);
  const partsB = parse(b);
  const len = Math.max(partsA.length, partsB.length);
  for (let i = 0; i < len; i++) {
    const diff = (partsA[i] ?? 0) - (partsB[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/**
 * Entries strictly newer than `since`. Used by the onboarding wizard to
 * render only what's changed since the user's last accepted version.
 *
 * `since === null` means "first install ever" — return an empty list so
 * fresh users don't see "What's New" before they've experienced the old
 * thing. The wizard falls back to its Welcome step in that case.
 */
export function changelogSince(since: string | null): ChangelogEntry[] {
  if (since === null) return [];
  return CHANGELOG.filter((entry) => compareSemver(entry.version, since) > 0);
}
