import { chmod, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";

const DEFAULT_ACTION_TTL_MS = 24 * 60 * 60 * 1000;

async function chmodBestEffort(filePath, mode) {
  try {
    await chmod(filePath, mode);
  } catch (error) {
    if (process.platform !== "win32") throw error;
  }
}

export class ThreadStore {
  static async open(filePath, options = {}) {
    const store = new ThreadStore(filePath, options);
    await store.load();
    return store;
  }

  constructor(filePath, options = {}) {
    this.filePath = filePath;
    this.options = {
      messageLimit: options.messageLimit || 0,
      actions: options.actions === true,
      actionLimit: options.actionLimit || 200,
      actionTtlMs: options.actionTtlMs || DEFAULT_ACTION_TTL_MS,
      privateMode: options.privateMode === true
    };
    this.data = { chats: {} };
    this.ensureShape();
  }

  ensureShape() {
    if (!this.data || typeof this.data !== "object") this.data = {};
    if (!this.data.chats || typeof this.data.chats !== "object") this.data.chats = {};
    if (this.options.messageLimit > 0 && !Array.isArray(this.data.messages)) {
      this.data.messages = [];
    }
    if (this.options.actions && (!this.data.actions || typeof this.data.actions !== "object")) {
      this.data.actions = {};
    }
  }

  async load() {
    try {
      const raw = await readFile(this.filePath, "utf8");
      this.data = JSON.parse(raw);
      this.ensureShape();
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }

  async recordMessage(messageKey) {
    if (!messageKey || this.options.messageLimit <= 0) return false;
    this.ensureShape();
    if (this.data.messages.includes(messageKey)) return true;
    this.data.messages.push(messageKey);
    this.data.messages = this.data.messages.slice(-this.options.messageLimit);
    await this.save();
    return false;
  }

  async getChat(chatId) {
    return this.data.chats[chatId] || null;
  }

  listChats() {
    return Object.entries(this.data.chats || {});
  }

  async setChat(chatId, state) {
    this.data.chats[chatId] = state;
    await this.save();
    return state;
  }

  async patchChat(chatId, patch) {
    const current = this.data.chats[chatId] || {};
    this.data.chats[chatId] = { ...current, ...patch };
    await this.save();
    return this.data.chats[chatId];
  }

  async putAction(action) {
    if (!this.options.actions) return "";
    this.ensureShape();
    const token = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
    this.data.actions[token] = {
      ...action,
      createdAt: new Date().toISOString()
    };
    this.pruneActions();
    await this.save();
    return token;
  }

  async getAction(token) {
    if (!token || !this.options.actions) return null;
    this.ensureShape();
    return this.data.actions[token] || null;
  }

  async takeAction(token) {
    const action = await this.getAction(token);
    if (action) {
      delete this.data.actions[token];
      await this.save();
    }
    return action;
  }

  pruneActions() {
    if (!this.options.actions) return;
    const cutoff = Date.now() - this.options.actionTtlMs;
    const fresh = Object.entries(this.data.actions || {}).filter(([, action]) => {
      const time = Date.parse(action.createdAt || "");
      return Number.isFinite(time) && time >= cutoff;
    });
    this.data.actions = Object.fromEntries(fresh.slice(-this.options.actionLimit));
  }

  async save() {
    const dir = path.dirname(this.filePath);
    await mkdir(dir, { recursive: true, mode: 0o700 });
    if (this.options.privateMode) await chmodBestEffort(dir, 0o700);
    const tmp = `${this.filePath}.tmp`;
    await writeFile(tmp, `${JSON.stringify(this.data, null, 2)}\n`, { mode: 0o600 });
    if (this.options.privateMode) await chmodBestEffort(tmp, 0o600);
    await rename(tmp, this.filePath);
    if (this.options.privateMode) await chmodBestEffort(this.filePath, 0o600);
  }
}

export function envFirst(env, ...names) {
  for (const name of names) {
    const value = env?.[name];
    if (value != null && String(value).trim()) return String(value).trim();
  }
  return "";
}

export function parseList(raw) {
  return String(raw || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function parseBool(raw, fallback = false) {
  if (raw == null || raw === "") return fallback;
  return ["1", "true", "yes", "on"].includes(String(raw).trim().toLowerCase());
}

export function parseEnvText(raw) {
  const env = {};
  for (const line of String(raw || "").split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const normalized = trimmed.startsWith("export ") ? trimmed.slice(7).trim() : trimmed;
    const index = normalized.indexOf("=");
    if (index <= 0) continue;
    const key = normalized.slice(0, index).trim();
    let value = normalized.slice(index + 1).trim();
    if (
      value.length >= 2 &&
      ((value.startsWith('"') && value.endsWith('"')) ||
        (value.startsWith("'") && value.endsWith("'")))
    ) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }
  return env;
}

export function cleanEnvValue(value) {
  return String(value ?? "").trim();
}

export function isPlaceholderValue(value) {
  const normalized = cleanEnvValue(value).toLowerCase();
  return (
    !normalized ||
    normalized.includes("replace-with") ||
    normalized.includes("xxxxxxxx") ||
    normalized === "changeme"
  );
}

export function parseTextContent(content, keys = ["text", "content"]) {
  if (typeof content !== "string") return "";
  try {
    const parsed = JSON.parse(content);
    for (const key of keys) {
      if (typeof parsed?.[key] === "string") return parsed[key];
    }
  } catch {
    return content;
  }
  return content;
}

export function stripGroupPrefix(text, { chatType, requirePrefix, prefix, directChatTypes = [] }) {
  const trimmed = String(text || "").trim();
  if (!trimmed) return { accepted: false, text: "" };
  if (!requirePrefix || directChatTypes.includes(chatType)) {
    return { accepted: true, text: trimmed };
  }
  const marker = prefix || "/ds";
  if (trimmed === marker) return { accepted: true, text: "/help" };
  if (trimmed.startsWith(`${marker} `)) {
    return { accepted: true, text: trimmed.slice(marker.length).trim() };
  }
  return { accepted: false, text: "" };
}

export function parseCommand(text, options = {}) {
  const trimmed = String(text || "").trim();
  if (!trimmed.startsWith("/")) return { name: "prompt", args: trimmed };
  const [head, ...rest] = trimmed.split(/\s+/);
  const rawName = head.slice(1);
  const name = (options.stripBotMention ? rawName.split("@")[0] : rawName).toLowerCase();
  return {
    name,
    args: rest.join(" ").trim()
  };
}

export function parseApprovalDecisionArgs(args) {
  const parts = String(args || "")
    .split(/\s+/)
    .filter(Boolean);
  return {
    approvalId: parts[0] || "",
    remember: parts.slice(1).includes("remember")
  };
}

export function commandAction(command, options = {}) {
  const allowMenu = options.allowMenu === true;
  const allowStart = options.allowStart === true;
  switch (command.name) {
    case "start":
      if (allowStart) return { kind: "help" };
      break;
    case "help":
      return { kind: "help" };
    case "menu":
      if (allowMenu) return { kind: "menu" };
      break;
    case "status":
      return { kind: "status" };
    case "threads":
      return { kind: "threads" };
    case "new":
      return { kind: "new_thread" };
    case "resume":
      return { kind: "resume", threadId: command.args };
    case "interrupt":
      return { kind: "interrupt" };
    case "compact":
      return { kind: "compact" };
    case "model":
      return { kind: "set_model", modelName: command.args };
    case "allow":
      return { kind: "approval", decision: "allow", ...parseApprovalDecisionArgs(command.args) };
    case "deny":
      return { kind: "approval", decision: "deny", ...parseApprovalDecisionArgs(command.args) };
    case "prompt":
      return { kind: "prompt", prompt: command.args };
    default:
      break;
  }
  return {
    kind: "prompt",
    prompt: `/${command.name}${command.args ? ` ${command.args}` : ""}`
  };
}

export function preservedChatStateFields(state = {}, fields = ["model"]) {
  const preserved = {};
  for (const field of fields) {
    if (Object.prototype.hasOwnProperty.call(state || {}, field)) {
      preserved[field] = state[field] || null;
    }
  }
  return preserved;
}

export function splitMessage(text, maxChars = 3500) {
  const value = String(text || "");
  const chars = Array.from(value);
  if (chars.length <= maxChars) return value ? [value] : [];
  const chunks = [];
  let cursor = 0;
  while (cursor < chars.length) {
    chunks.push(chars.slice(cursor, cursor + maxChars).join(""));
    cursor += maxChars;
  }
  return chunks;
}

export function compactRuntimeError(status, body) {
  const message =
    body?.error?.message ||
    body?.message ||
    (typeof body === "string" ? body : JSON.stringify(body));
  return `Runtime API request failed (${status}): ${message}`;
}

export function latestRunningTurn(detail) {
  const turns = Array.isArray(detail?.turns) ? detail.turns : [];
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index];
    if (["queued", "in_progress"].includes(turn?.status)) return turn;
  }
  return null;
}

export function activeTurnBlock(detail, state = {}) {
  const runningTurn = latestRunningTurn(detail);
  if (!runningTurn) return null;
  const activeTurnId = state?.activeTurnId || "";
  return {
    turnId: runningTurn.id || activeTurnId,
    message: `Thread already has active turn ${
      runningTurn.id || activeTurnId || "(unknown)"
    }. Wait for it to finish or send /interrupt.`
  };
}
