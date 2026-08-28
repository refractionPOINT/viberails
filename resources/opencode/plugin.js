// @__PROJECT_NAME__-plugin -- generated file, do not edit; re-run `__PROJECT_NAME__ install`.
//
// Bridges OpenCode's in-process plugin hooks to the callback binary.
//
// OpenCode has no shell-command hook, so this shim spawns the binary with the
// callback subcommand, writes the event as one line of JSON on stdin and reads
// the decision back from stdout. That keeps the wire protocol identical to
// every other provider.
//
// The name and binary path below are substituted at install time; see
// render_plugin in src/providers/opencode.rs.

import { spawn } from "node:child_process";

const PLUGIN_NAME = "__PROJECT_NAME__";
const CALLBACK_BIN = __CALLBACK_BIN__;
const CALLBACK_ARG = "opencode-callback";

// Both budgets must exceed the binary's own cloud timeout (10s), otherwise a
// slow cloud is killed mid-request here: the authorization would fail open
// before the binary could answer, and the audit event would be lost outright.
const AUTHORIZE_TIMEOUT_MS = 30000;
const NOTIFY_TIMEOUT_MS = 15000;

const log = (message) => console.error(`[${PLUGIN_NAME}] ${message}`);

/// Send one event to the binary and resolve with the parsed decision.
/// Resolves null when the callback cannot be reached, exits non-zero, times out
/// or writes something that isn't JSON. A null decision means "allow": the hook
/// fails open, as the OpenClaw plugin does on the same failures.
function callCallback(payload, timeoutMs) {
  return new Promise((resolve) => {
    let child;

    try {
      child = spawn(CALLBACK_BIN, [CALLBACK_ARG], {
        stdio: ["pipe", "pipe", "pipe"],
      });
    } catch (err) {
      log(`Spawn error: ${err.message}`);
      resolve(null);
      return;
    }

    let stdout = "";
    let stderr = "";
    let settled = false;

    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(value);
    };

    const timer = setTimeout(() => {
      log(`Callback timed out after ${timeoutMs}ms`);
      try {
        child.kill("SIGKILL");
      } catch {
        /* already gone */
      }
      finish(null);
    }, timeoutMs);

    // Decode as UTF-8 so a multi-byte character split across two chunks is not
    // corrupted before JSON.parse sees it.
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");

    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });

    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    child.on("error", (err) => {
      log(`Spawn error: ${err.message}`);
      finish(null);
    });

    child.on("close", (code) => {
      // A spawn error or timeout already reported and settled this call.
      if (settled) return;

      if (code !== 0) {
        log(`Process exited with code ${code}: ${stderr}`);
        finish(null);
        return;
      }

      // An empty response is the approve case: exit 0 with no output.
      const trimmed = stdout.trim();
      if (!trimmed) {
        finish(null);
        return;
      }

      try {
        finish(JSON.parse(trimmed));
      } catch {
        log(`Unparseable response: ${trimmed}`);
        finish(null);
      }
    });

    try {
      child.stdin.write(`${JSON.stringify(payload)}\n`);
      child.stdin.end();
    } catch (err) {
      log(`Failed to send payload: ${err.message}`);
      finish(null);
    }
  });
}

export const plugin = async ({ directory, worktree }) => {
  return {
    // Awaited before the tool runs. Throwing rejects the call and surfaces the
    // reason to the model, which is how a block is enforced in OpenCode.
    "tool.execute.before": async (input, output) => {
      const decision = await callCallback(
        {
          hook_event_name: "tool.execute.before",
          session_id: input.sessionID,
          call_id: input.callID,
          tool_name: input.tool,
          tool_input: output.args ?? {},
          cwd: directory,
          worktree,
        },
        AUTHORIZE_TIMEOUT_MS,
      );

      if (decision && decision.decision === "block") {
        throw new Error(decision.reason || `blocked by ${PLUGIN_NAME} policy`);
      }
    },

    // session.idle marks the end of an assistant turn; forwarded for auditing.
    event: async ({ event }) => {
      if (event.type !== "session.idle") return;

      await callCallback(
        {
          hook_event_name: "session.idle",
          session_id: event.properties?.sessionID,
          cwd: directory,
          worktree,
        },
        NOTIFY_TIMEOUT_MS,
      );
    },
  };
};
