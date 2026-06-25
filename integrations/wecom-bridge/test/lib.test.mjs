import assert from "node:assert/strict";
import { mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { ThreadStore, validateBridgeConfig } from "../src/lib.mjs";

test("ThreadStore writes private state files", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "codewhale-wecom-"));
  try {
    const statePath = path.join(dir, "nested", "thread-map.json");
    const store = await ThreadStore.open(statePath);

    await store.setChat("single:user-a", {
      threadId: "thread-a",
      lastSeq: 1,
      activeTurnId: null
    });

    const saved = await ThreadStore.open(statePath);
    assert.equal((await saved.getChat("single:user-a")).threadId, "thread-a");

    if (process.platform !== "win32") {
      assert.equal((await stat(path.dirname(statePath))).mode & 0o777, 0o700);
      assert.equal((await stat(statePath)).mode & 0o777, 0o600);
    }
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("validateBridgeConfig rejects placeholder secrets", () => {
  const result = validateBridgeConfig({
    WECOM_BOT_ID: "your-bot-id",
    WECOM_BOT_SECRET: "your-bot-secret",
    CODEWHALE_RUNTIME_TOKEN: "replace-with-long-random-token",
    CODEWHALE_RUNTIME_URL: "http://127.0.0.1:7878"
  });

  assert.equal(result.ok, false);
  assert.deepEqual(
    result.errors.map((item) => item.code),
    ["placeholder_runtime_token"]
  );
});
