export const ALL_USAGE_PROVIDER_ID = "all";

export type UsageProvider = string;
export type RateLimitProviderId = string;
export type UsagePeriod = "5h" | "day" | "week" | "month" | "year";
export type DefaultPeriod = Exclude<UsagePeriod, "year">;
export type UsageSource = "ccusage" | "parser" | "mixed";

export interface HeaderTabConfig {
  label: string;
  enabled: boolean;
}

export type HeaderTabs = Record<string, HeaderTabConfig>;

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
  usage_source: UsageSource;
  usage_warning: string | null;
  period_label: string;
  has_earlier_data: boolean;
  change_stats: ChangeStats | null;
  subagent_stats: SubagentStats | null;
  device_breakdown: DeviceSummary[] | null;
  device_chart_buckets: ChartBucket[] | null;
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

export interface ScopeModelUsage {
  display_name: string;
  model_key: string;
  cost: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
}

export interface ScopeUsageSummary {
  cost: number;
  tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  session_count: number;
  pct_of_total_cost: number | null;
  top_models: ScopeModelUsage[];
  added_lines: number;
  removed_lines: number;
}

export interface SubagentStats {
  main: ScopeUsageSummary;
  subagents: ScopeUsageSummary;
}

export type AccordionToggleDetail = {
  durationMs: number;
  expanding: boolean;
  scope: "main" | "subagents" | "devices";
};

export type ChartDetailToggleDetail = {
  durationMs: number;
  fromHeight: number;
  toHeight: number;
};

export interface CalendarDay {
  day: number;
  cost: number;
}

export interface MonthlyUsagePayload {
  year: number;
  month: number;
  days: CalendarDay[];
  total_cost: number;
  usage_source: UsageSource;
  usage_warning: string | null;
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

export type RateLimitsPayload = Record<RateLimitProviderId, ProviderRateLimits | null>;

export type BarDisplay = 'off' | 'single' | 'both';
export type PercentageFormat = 'compact' | 'verbose';
export type CostPrecision = 'whole' | 'full';

export interface TrayConfig {
  barDisplay: BarDisplay;
  barProvider: RateLimitProviderId;
  showPercentages: boolean;
  percentageFormat: PercentageFormat;
  showCost: boolean;
  costPrecision: CostPrecision;
}

export interface StatusWidgetSummary {
  config: TrayConfig;
  totalCost: number;
  claudeUtil: number | null;
  codexUtil: number | null;
  title: string;
}

// ── SSH / Device usage ──

export interface SshHostInfo {
  alias: string;
  hostname: string;
  user: string | null;
  port: number;
}

export interface SshHostConfig {
  alias: string;
  enabled: boolean;
  include_in_stats?: boolean;
}

export interface SshHostStatus {
  alias: string;
  enabled: boolean;
  lastSync: string | null;
  lastError: string | null;
  fileCount: number;
}

export interface SshTestResult {
  success: boolean;
  message: string;
  durationMs: number;
}

export interface SshSyncResult {
  testSuccess: boolean;
  testMessage: string;
  testDurationMs: number;
  recordsSynced: number;
}

export interface DeviceModelSummary {
  display_name: string;
  model_key: string;
  cost: number;
  tokens: number;
}

export interface DeviceSummary {
  device: string;
  total_cost: number;
  total_tokens: number;
  model_breakdown: DeviceModelSummary[];
  is_local: boolean;
  status: "online" | "offline" | "syncing" | "error";
  last_synced: string | null;
  error_message: string | null;
  cost_percentage: number;
  include_in_stats: boolean;
}

export interface DeviceUsagePayload {
  devices: DeviceSummary[];
  total_cost: number;
  chart_buckets: ChartBucket[];
  last_updated: string;
  period_label: string;
}

export type ChartSegmentMode = "model" | "device";

export type FloatBallExpandDirection = "left" | "right";

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

export type RateLimitsMonitorState = Record<RateLimitProviderId, RateLimitProviderMonitorState>;
