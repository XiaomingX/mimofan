export type FleetRunId = string;
export type FleetRunStatus =
  | "pending"
  | "queued"
  | "running"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export type FleetWorkerStatus =
  | "unknown"
  | "online"
  | "busy"
  | "offline"
  | "unhealthy"
  | "draining"
  | "retired";

export type FleetArtifactKind =
  | "log"
  | "patch"
  | "test_result"
  | "report"
  | "checkpoint"
  | "receipt"
  | string;

export interface FleetStatusSummary {
  runs: number;
  queued: number;
  running: number;
  completed: number;
  partial: number;
  failed: number;
  restarted: number;
  escalated: number;
  transport_failed: number;
  task_failed: number;
  verifier_failed: number;
  cancelled: number;
  stale: number;
  workers: Record<string, FleetWorkerStatus>;
}

export interface FleetTaskStatusSummary {
  task_id: string;
  status: "enqueued" | "leased" | "completed" | "failed" | "cancelled";
  leased_to?: string | null;
  attempts: number;
}

export interface FleetRunSummary {
  id: string;
  name: string;
  status: FleetStatusSummary;
  task_count: number;
  worker_count: number;
  tasks: FleetTaskStatusSummary[];
  labels: Record<string, string>;
  created_at: string;
  updated_at?: string | null;
  completed_at?: string | null;
}

export interface FleetRunDetail extends FleetRunSummary {
  task_specs: FleetTaskSpec[];
  worker_specs: FleetWorkerSpec[];
}

export interface FleetRunsResponse {
  status: FleetStatusSummary;
  runs: FleetRunSummary[];
}

export interface FleetTaskSpec {
  id: string;
  name: string;
  description?: string | null;
  objective?: string | null;
  instructions: string;
  worker?: FleetTaskWorkerProfile | null;
  workspace?: FleetWorkspaceRequirements | null;
  input_files?: string[];
  context?: string[];
  budget?: FleetTaskBudget | null;
  tags?: string[];
  expected_artifacts?: FleetArtifactKind[];
  scorer?: Record<string, unknown> | null;
  retry_policy?: Record<string, unknown> | null;
  alert_policy?: FleetAlertPolicy | null;
  timeout_seconds?: number | null;
  metadata?: Record<string, unknown>;
}

export interface FleetTaskWorkerProfile {
  role?: string | null;
  tool_profile?: string | null;
  tools?: string[];
  capabilities?: string[];
}

export interface FleetWorkspaceRequirements {
  root?: string | null;
  required_files?: string[];
  writable_paths?: string[];
  environment?: FleetEnvironmentRequirements | null;
}

export interface FleetEnvironmentRequirements {
  required?: string[];
  allowlist?: string[];
}

export interface FleetTaskBudget {
  max_tokens?: number | null;
  max_tool_calls?: number | null;
  max_seconds?: number | null;
}

export interface FleetAlertPolicy {
  events?: string[];
  channels?: Array<Record<string, unknown>>;
  after_attempts?: number | null;
  after_minutes_stale?: number | null;
}

export interface FleetWorkerSpec {
  id: string;
  name: string;
  host: Record<string, unknown>;
  labels?: Record<string, string>;
  capabilities?: string[];
  max_concurrent_tasks?: number | null;
}

export interface FleetArtifactRef {
  kind: FleetArtifactKind;
  path: string;
  checksum?: string | null;
  mime_type?: string | null;
  size_bytes?: number | null;
}

export type FleetWorkerEventPayload =
  | { state: "queued" }
  | { state: "leased"; lease_expires_at?: string | null }
  | { state: "starting" }
  | { state: "running" }
  | { state: "model_wait"; model?: string | null }
  | { state: "running_tool"; tool: string; call_id?: string | null }
  | { state: "heartbeat"; cpu_percent?: number | null; memory_mb?: number | null }
  | ({ state: "artifact" } & FleetArtifactRef)
  | { state: "completed"; exit_code?: number | null; summary?: string | null }
  | { state: "failed"; reason: string; recoverable?: boolean }
  | { state: "cancelled"; cancelled_by?: string | null }
  | { state: "interrupted"; signal?: string | null }
  | { state: "stale"; last_heartbeat_at?: string | null }
  | { state: "restarted"; restart_count?: number }
  | { state: "escalated"; channel: string; alert_id?: string | null };

export interface FleetWorkerEvent {
  seq: number;
  run_id: string;
  worker_id: string;
  task_id: string;
  timestamp: string;
  label?: string;
  payload: FleetWorkerEventPayload;
  extra?: Record<string, unknown>;
}

export interface FleetWorkerInspection {
  worker_id: string;
  status: FleetWorkerStatus;
  run_id?: string | null;
  task_id?: string | null;
  objective?: string | null;
  role?: string | null;
  host?: Record<string, unknown> | null;
  latest_heartbeat_at?: string | null;
  latest_event?: FleetWorkerEvent | null;
  artifacts: FleetArtifactRef[];
  last_error?: string | null;
  alert_state?: Record<string, unknown> | null;
}

export interface FleetWorkersResponse {
  run_id: string;
  workers: FleetWorkerInspection[];
}

export interface FleetWorkerActionResponse {
  action: "interrupt" | "restart";
  worker: FleetWorkerInspection;
}

export interface StopFleetRunResponse {
  action: "stop";
  run_id: string;
  stopped: number;
  status: FleetStatusSummary;
}

export interface RuntimeClientOptions {
  baseUrl?: string;
  token?: string;
  fetch?: typeof fetch;
}

export interface FleetRunCreateSpec {
  name?: string;
  task_specs?: FleetTaskSpec[];
  worker_specs?: FleetWorkerSpec[];
  labels?: Record<string, string>;
}

export class RuntimeApiError extends Error {
  status?: number;
  method?: string;
  path?: string;
  body?: string;
}

export class RuntimeCapabilityError extends RuntimeApiError {
  capability: string;
}

export class MimofanRuntimeClient {
  constructor(options?: RuntimeClientOptions);
  createFleetRun(spec: FleetRunCreateSpec | Record<string, unknown>): Promise<unknown>;
  listFleetRuns(): Promise<FleetRunsResponse>;
  getFleetRun(runId: FleetRunId): Promise<FleetRunDetail>;
  listFleetWorkers(runId: FleetRunId): Promise<FleetWorkersResponse>;
  getFleetWorker(workerId: string): Promise<FleetWorkerInspection>;
  interruptWorker(workerId: string): Promise<FleetWorkerActionResponse>;
  restartWorker(workerId: string): Promise<FleetWorkerActionResponse>;
  stopFleetRun(runId: FleetRunId): Promise<StopFleetRunResponse>;
  fleetEvents(
    runId: FleetRunId,
    options?: { path?: string },
  ): AsyncIterable<FleetWorkerEvent>;
}

export function createRuntimeClient(options?: RuntimeClientOptions): MimofanRuntimeClient;
