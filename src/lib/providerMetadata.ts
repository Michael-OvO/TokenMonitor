import {
  ALL_USAGE_PROVIDER_ID,
  type HeaderTabs,
  type RateLimitProviderId,
  type RateLimitWindow,
  type UsageProvider,
} from "./types/index.js";

export { ALL_USAGE_PROVIDER_ID } from "./types/index.js";

export type UsageProviderLogoKind = "all" | "claude" | "codex" | "cursor" | "generic";
type UsageProviderBrandColor = readonly [red: number, green: number, blue: number];
type RateLimitUtilizationLabelFormat = "percent" | "percent_used";

interface RateLimitProviderDefinition {
  cacheFile: string;
  minFetchIntervalMs: number;
  primaryWindowId: string;
  utilizationLabelFormat: RateLimitUtilizationLabelFormat;
  idleSummary: string;
  missingMetadataErrors?: readonly string[];
  fallbackWindow?: Readonly<RateLimitWindow>;
  expiredWindowGraceMs?: number;
  preservePeakUtilization?: boolean;
}

interface UsageProviderDefinition {
  id: UsageProvider;
  label: string;
  title: string;
  logoKind: UsageProviderLogoKind;
  brandColor?: UsageProviderBrandColor;
  planTierCosts?: Readonly<Record<string, number>>;
  supportsRateLimits: boolean;
  rateLimits?: RateLimitProviderDefinition;
}

const ALL_PROVIDER_DEFINITION: UsageProviderDefinition = {
  id: ALL_USAGE_PROVIDER_ID,
  label: "All",
  title: "All",
  logoKind: "all",
  supportsRateLimits: false,
};

const USAGE_INTEGRATION_DEFINITIONS: UsageProviderDefinition[] = [
  {
    id: "claude",
    label: "Claude",
    title: "Claude Code",
    logoKind: "claude",
    brandColor: [196, 112, 75],
    planTierCosts: {
      Pro: 20,
      "Max 5x": 100,
      "Max 20x": 200,
      Free: 0,
    },
    supportsRateLimits: true,
    rateLimits: {
      cacheFile: "rate-limits-claude.json",
      // Pre-rewrite this was 5 minutes — a budget guard for the OAuth API
      // path where each fetch counted against Anthropic's rate-limit
      // budget. The current fetch is a local statusline-events-file read
      // plus arithmetic, so there's nothing to conserve. Setting to 0
      // closes the gap where the tray updated within 2s of a CC prompt
      // but the dashboard sat on the throttled cache for up to 5 min.
      minFetchIntervalMs: 0,
      primaryWindowId: "five_hour",
      utilizationLabelFormat: "percent",
      idleSummary: "No active rate limit windows were returned for this provider.",
    },
  },
  {
    id: "codex",
    label: "Codex",
    title: "Codex",
    logoKind: "codex",
    brandColor: [74, 123, 157],
    planTierCosts: {
      Plus: 20,
      Pro: 200,
      Free: 0,
    },
    supportsRateLimits: true,
    rateLimits: {
      cacheFile: "rate-limits-codex.json",
      minFetchIntervalMs: 0,
      primaryWindowId: "primary",
      utilizationLabelFormat: "percent_used",
      idleSummary: "Usage is being recorded, but this Codex session has not emitted rate-limit metadata yet.",
      missingMetadataErrors: [
        "no codex session files found",
        "no rate limit data in codex session files",
      ],
      fallbackWindow: {
        windowId: "primary",
        label: "Session (5hr)",
        utilization: 0,
        resetsAt: null,
      },
      expiredWindowGraceMs: 60_000,
      preservePeakUtilization: true,
    },
  },
  {
    id: "cursor",
    label: "Cursor",
    title: "Cursor IDE",
    logoKind: "cursor",
    brandColor: [92, 106, 196],
    supportsRateLimits: true,
    rateLimits: {
      cacheFile: "rate-limits-cursor.json",
      minFetchIntervalMs: 300_000,
      primaryWindowId: "auto_composer",
      utilizationLabelFormat: "percent_used",
      idleSummary: "No Cursor usage data available. Make sure Cursor IDE is signed in on this machine.",
    },
  },
];

export const DEFAULT_USAGE_PROVIDER = USAGE_INTEGRATION_DEFINITIONS[0]?.id ?? ALL_USAGE_PROVIDER_ID;

export const USAGE_PROVIDER_ORDER: UsageProvider[] = [
  ALL_PROVIDER_DEFINITION.id,
  ...USAGE_INTEGRATION_DEFINITIONS.map((definition) => definition.id),
];

export const RATE_LIMIT_PROVIDER_ORDER: RateLimitProviderId[] = USAGE_INTEGRATION_DEFINITIONS
  .filter((definition) => definition.supportsRateLimits)
  .map((definition) => definition.id);

export const DEFAULT_RATE_LIMIT_PROVIDER = RATE_LIMIT_PROVIDER_ORDER[0] ?? DEFAULT_USAGE_PROVIDER;

const PROVIDER_DEFINITIONS = [ALL_PROVIDER_DEFINITION, ...USAGE_INTEGRATION_DEFINITIONS];
const PROVIDER_DEFINITION_MAP = new Map(
  PROVIDER_DEFINITIONS.map((definition) => [definition.id, definition] as const),
);
const USAGE_PROVIDER_ID_SET = new Set(USAGE_PROVIDER_ORDER);
const RATE_LIMIT_PROVIDER_ID_SET = new Set<string>(RATE_LIMIT_PROVIDER_ORDER);

function usageProviderDefinition(provider: UsageProvider): UsageProviderDefinition | undefined {
  return PROVIDER_DEFINITION_MAP.get(provider);
}

function rateLimitProviderDefinition(provider: RateLimitProviderId): RateLimitProviderDefinition {
  const definition = usageProviderDefinition(provider)?.rateLimits;
  if (!definition) {
    throw new Error(`No rate-limit provider metadata registered for "${provider}"`);
  }
  return definition;
}

export function createDefaultHeaderTabs(): HeaderTabs {
  const headerTabs: HeaderTabs = {};
  for (const definition of PROVIDER_DEFINITIONS) {
    headerTabs[definition.id] = {
      label: definition.label,
      enabled: true,
    };
  }
  return headerTabs;
}

export function isUsageProvider(value: unknown): value is UsageProvider {
  return typeof value === "string" && USAGE_PROVIDER_ID_SET.has(value);
}

export function isRateLimitProvider(value: unknown): value is RateLimitProviderId {
  return typeof value === "string" && RATE_LIMIT_PROVIDER_ID_SET.has(value);
}

export function getUsageProviderLabel(provider: UsageProvider): string {
  return PROVIDER_DEFINITION_MAP.get(provider)?.label ?? provider;
}

export function getUsageProviderTitle(provider: UsageProvider): string {
  return PROVIDER_DEFINITION_MAP.get(provider)?.title ?? getUsageProviderLabel(provider);
}

export function getUsageProviderLogoKind(provider: UsageProvider): UsageProviderLogoKind {
  return PROVIDER_DEFINITION_MAP.get(provider)?.logoKind ?? "generic";
}

export function getUsageProviderBrandColor(
  provider: UsageProvider,
  opacity = 1,
): string | null {
  const rgb = usageProviderDefinition(provider)?.brandColor;
  if (!rgb) return null;
  const alpha = Math.max(0, Math.min(opacity, 1));
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${alpha})`;
}

export function getUsageProviderPlanTierCost(
  provider: UsageProvider,
  tier: string | null | undefined,
): number {
  if (!tier) return 0;
  return usageProviderDefinition(provider)?.planTierCosts?.[tier] ?? 0;
}

export function getAdjacentWarmProviders(provider: UsageProvider): UsageProvider[] {
  return USAGE_INTEGRATION_DEFINITIONS
    .map((definition) => definition.id)
    .filter((candidate) => candidate !== provider);
}

export function rateLimitProvidersForScope(scope: UsageProvider): RateLimitProviderId[] {
  if (scope === ALL_USAGE_PROVIDER_ID) return [...RATE_LIMIT_PROVIDER_ORDER];
  return isRateLimitProvider(scope) ? [scope] : [];
}

export function getRateLimitCacheFile(provider: RateLimitProviderId): string {
  return rateLimitProviderDefinition(provider).cacheFile;
}

export function getRateLimitMinFetchIntervalMs(provider: RateLimitProviderId): number {
  return rateLimitProviderDefinition(provider).minFetchIntervalMs;
}

export function getRateLimitPrimaryWindowId(provider: RateLimitProviderId): string {
  return rateLimitProviderDefinition(provider).primaryWindowId;
}

export function getRateLimitExpiredWindowGraceMs(provider: RateLimitProviderId): number {
  return rateLimitProviderDefinition(provider).expiredWindowGraceMs ?? 0;
}

export function getRateLimitFallbackWindow(provider: RateLimitProviderId): RateLimitWindow | null {
  const fallbackWindow = rateLimitProviderDefinition(provider).fallbackWindow;
  return fallbackWindow ? { ...fallbackWindow } : null;
}

export function isRateLimitMissingMetadataError(
  provider: RateLimitProviderId,
  error: string | null,
): boolean {
  const markers = rateLimitProviderDefinition(provider).missingMetadataErrors ?? [];
  if (!error) return getRateLimitFallbackWindow(provider) !== null;

  const normalized = error.toLowerCase();
  return markers.some((marker) => normalized.includes(marker));
}

export function formatRateLimitUtilizationLabel(
  provider: RateLimitProviderId,
  utilization: number,
): string {
  // CC's statusline payload occasionally ships float `used_percentage`
  // values (e.g. `21.4632984` for the seven-day window). Rendering the raw
  // float produces a wall of decimals; integer rounding throws away real
  // signal at low utilizations. Round to one decimal place — accurate to
  // ±0.05% — and drop a trailing `.0` so clean integer values still
  // render as integers ("21%" not "21.0%").
  const rounded = Math.round(utilization * 10) / 10;
  const text = Number.isInteger(rounded) ? `${rounded}` : rounded.toFixed(1);
  if (rateLimitProviderDefinition(provider).utilizationLabelFormat === "percent_used") {
    return `${text}% used`;
  }
  return `${text}%`;
}

export function getRateLimitIdleSummary(provider: RateLimitProviderId): string {
  return rateLimitProviderDefinition(provider).idleSummary;
}

export function shouldPreservePeakRateLimitUtilization(provider: RateLimitProviderId): boolean {
  return Boolean(rateLimitProviderDefinition(provider).preservePeakUtilization);
}
