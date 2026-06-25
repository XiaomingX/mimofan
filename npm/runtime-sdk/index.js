const DEFAULT_BASE_URL = "http://127.0.0.1:7878";

export class RuntimeApiError extends Error {
  constructor(message, options = {}) {
    super(message);
    this.name = "RuntimeApiError";
    this.status = options.status;
    this.method = options.method;
    this.path = options.path;
    this.body = options.body;
  }
}

export class RuntimeCapabilityError extends RuntimeApiError {
  constructor(capability, message, options = {}) {
    super(message, options);
    this.name = "RuntimeCapabilityError";
    this.capability = capability;
  }
}

export class CodeWhaleRuntimeClient {
  constructor(options = {}) {
    this.baseUrl = normalizeBaseUrl(options.baseUrl ?? DEFAULT_BASE_URL);
    this.token = options.token ?? null;
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    if (typeof this.fetchImpl !== "function") {
      throw new TypeError("CodeWhaleRuntimeClient requires a fetch implementation");
    }
  }

  async createFleetRun(spec) {
    return this.#jsonRequest("/v1/fleet/runs", {
      method: "POST",
      body: spec,
      capability: "fleet_run_create",
    });
  }

  async listFleetRuns() {
    return this.#jsonRequest("/v1/fleet/runs");
  }

  async getFleetRun(runId) {
    return this.#jsonRequest(`/v1/fleet/runs/${segment(runId)}`);
  }

  async listFleetWorkers(runId) {
    return this.#jsonRequest(`/v1/fleet/runs/${segment(runId)}/workers`);
  }

  async getFleetWorker(workerId) {
    return this.#jsonRequest(`/v1/fleet/workers/${segment(workerId)}`);
  }

  async interruptWorker(workerId) {
    return this.#jsonRequest(`/v1/fleet/workers/${segment(workerId)}/interrupt`, {
      method: "POST",
    });
  }

  async restartWorker(workerId) {
    return this.#jsonRequest(`/v1/fleet/workers/${segment(workerId)}/restart`, {
      method: "POST",
    });
  }

  async stopFleetRun(runId) {
    return this.#jsonRequest(`/v1/fleet/runs/${segment(runId)}/stop`, {
      method: "POST",
    });
  }

  async *fleetEvents(runId, options = {}) {
    const path = options.path ?? `/v1/fleet/runs/${segment(runId)}/events`;
    const response = await this.#rawRequest(path, {
      method: "GET",
      capability: "fleet_event_stream",
    });
    const contentType = response.headers.get("content-type") ?? "";
    if (contentType.includes("application/json")) {
      const payload = await response.json();
      const events = Array.isArray(payload) ? payload : (payload.events ?? []);
      for (const event of events) {
        yield event;
      }
      return;
    }
    if (!response.body) {
      throw new RuntimeApiError("Runtime API event response did not include a readable body", {
        method: "GET",
        path,
      });
    }
    for await (const event of parseEventStream(response.body)) {
      yield event;
    }
  }

  async #jsonRequest(path, options = {}) {
    const response = await this.#rawRequest(path, options);
    if (response.status === 204) {
      return null;
    }
    return response.json();
  }

  async #rawRequest(path, options = {}) {
    const method = options.method ?? "GET";
    const headers = new Headers(options.headers);
    headers.set("accept", options.accept ?? "application/json");
    if (this.token) {
      headers.set("authorization", `Bearer ${this.token}`);
    }
    const init = { method, headers };
    if (options.body !== undefined) {
      headers.set("content-type", "application/json");
      init.body = JSON.stringify(options.body);
    }

    const response = await this.fetchImpl(new URL(path, this.baseUrl), init);
    if (response.ok) {
      return response;
    }

    const body = await readErrorBody(response);
    const errorOptions = { status: response.status, method, path, body };
    if (options.capability && [404, 405, 501].includes(response.status)) {
      throw new RuntimeCapabilityError(
        options.capability,
        `Runtime API capability '${options.capability}' is not available at ${method} ${path}`,
        errorOptions,
      );
    }
    throw new RuntimeApiError(
      `Runtime API request failed (${response.status}) for ${method} ${path}`,
      errorOptions,
    );
  }
}

export function createRuntimeClient(options = {}) {
  return new CodeWhaleRuntimeClient(options);
}

function normalizeBaseUrl(value) {
  return value.endsWith("/") ? value : `${value}/`;
}

function segment(value) {
  if (value === null || value === undefined || String(value).trim() === "") {
    throw new TypeError("Runtime API path segment must be a non-empty value");
  }
  return encodeURIComponent(String(value));
}

async function readErrorBody(response) {
  try {
    const text = await response.text();
    return text.length > 4096 ? `${text.slice(0, 4096)}...` : text;
  } catch {
    return "";
  }
}

async function* parseEventStream(body) {
  const decoder = new TextDecoder();
  let buffer = "";
  for await (const chunk of body) {
    buffer += decoder.decode(chunk, { stream: true });
    let boundary;
    while ((boundary = buffer.indexOf("\n\n")) >= 0) {
      const frame = buffer.slice(0, boundary);
      buffer = buffer.slice(boundary + 2);
      const event = parseSseFrame(frame);
      if (event !== undefined) {
        yield event;
      }
    }
  }
  buffer += decoder.decode();
  const event = parseSseFrame(buffer);
  if (event !== undefined) {
    yield event;
  }
}

function parseSseFrame(frame) {
  const data = frame
    .split(/\r?\n/)
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice("data:".length).trimStart())
    .join("\n");
  if (!data || data === "[DONE]") {
    return undefined;
  }
  return JSON.parse(data);
}
