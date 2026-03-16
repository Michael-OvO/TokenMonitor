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
}

export interface ActiveBlock {
  cost: number;
  burn_rate_per_hour: number;
  projected_cost: number;
  is_active: boolean;
}
