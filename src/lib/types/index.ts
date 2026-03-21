export type UsageProvider = "all" | "claude" | "codex";
export type DefaultProvider = Exclude<UsageProvider, "all">;
export type UsagePeriod = "5h" | "day" | "week" | "month" | "year";
export type DefaultPeriod = Exclude<UsagePeriod, "year">;

export interface HeaderTabConfig {
  label: string;
  enabled: boolean;
}

export type HeaderTabs = Record<UsageProvider, HeaderTabConfig>;

export interface UsagePayload {
  total_cost: number;
  total_tokens: number;
  session_count: number;
  input_tokens: number;
  output_tokens: number;
  chart_buckets: ChartBucket[];
  model_breakdown: ModelSummary[];
  active_block: ActiveBlock | null;
  five_hour_cost: number;
  last_updated: string;
  from_cache: boolean;
  period_label: string;
  has_earlier_data: boolean;
  change_stats: ChangeStats | null;
}

export interface ChartBucket {
  label: string;
  sort_key?: string;
  total: number;
  segments: ChartSegment[];
}

export interface ChartSegment {
  model: string;
  model_key: string;
  cost: number;
  tokens: number;
}

export interface ModelSummary {
  display_name: string;
  model_key: string;
  cost: number;
  tokens: number;
  change_stats: ModelChangeSummary | null;
}

export interface KnownModel {
  display_name: string;
  model_key: string;
}

export interface ActiveBlock {
  cost: number;
  burn_rate_per_hour: number;
  projected_cost: number;
  is_active: boolean;
}

export interface ChangeStats {
  added_lines: number;
  removed_lines: number;
  net_lines: number;  // Can be negative — only signed integer field
  files_touched: number;
  change_events: number;
  write_events: number;
  code_lines_changed: number;
  docs_lines_changed: number;
  config_lines_changed: number;
  other_lines_changed: number;
  avg_lines_per_event: number | null;
  cost_per_100_net_lines: number | null;
  tokens_per_net_line: number | null;
  rewrite_ratio: number | null;
  churn_ratio: number | null;
  dominant_extension: string | null;
}

export interface ModelChangeSummary {
  added_lines: number;
  removed_lines: number;
  net_lines: number;
  files_touched: number;
  change_events: number;
}

export interface CalendarDay {
  day: number;
  cost: number;
}

export interface MonthlyUsagePayload {
  year: number;
  month: number;
  days: CalendarDay[];
  total_cost: number;
}

// ── Rate limits ──

export interface RateLimitWindow {
  windowId: string;
  label: string;
  utilization: number;
  resetsAt: string | null;
}

export interface ExtraUsageInfo {
  isEnabled: boolean;
  monthlyLimit: number;
  usedCredits: number;
  utilization: number | null;
}

export interface ProviderRateLimits {
  provider: string;
  planTier: string | null;
  windows: RateLimitWindow[];
  extraUsage: ExtraUsageInfo | null;
  stale: boolean;
  error: string | null;
  retryAfterSeconds: number | null;
  cooldownUntil: string | null;
  fetchedAt: string;
}

export interface RateLimitsPayload {
  claude: ProviderRateLimits | null;
  codex: ProviderRateLimits | null;
}

export type BarDisplay = 'off' | 'single' | 'both';
export type PercentageFormat = 'compact' | 'verbose';
export type CostPrecision = 'whole' | 'full';

export interface TrayConfig {
  barDisplay: BarDisplay;
  barProvider: DefaultProvider;
  showPercentages: boolean;
  percentageFormat: PercentageFormat;
  showCost: boolean;
  costPrecision: CostPrecision;
}

export interface RateLimitRequestState {
  loading: boolean;
  loaded: boolean;
  error: string | null;
  deferredUntil: string | null;
}

export interface RateLimitProviderMonitorState extends RateLimitRequestState {
  failureStreak: number;
  lastAttemptAt: string | null;
  lastSuccessAt: string | null;
}

export interface RateLimitsMonitorState {
  claude: RateLimitProviderMonitorState;
  codex: RateLimitProviderMonitorState;
}
