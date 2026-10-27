#!/usr/bin/env node
// scripts/mcp-repl.mjs — manual REPL for the Simple Post MCP daemon.
//
// Reads JSON-RPC frames (one per line) from stdin and forwards them to the
// running daemon at http://127.0.0.1:<port>/mcp, printing each response on
// stdout. Lets you poke at the 52 tools interactively:
//
//   npm run mcp:repl                   # uses port_registry to discover
//   npm run mcp:repl -- --port=41923   # explicit port (debug)
//
// Useful while developing new tools or debugging the proxy.

import process from 'node:process';
import { createInterface } from 'node:readline';
import { readFileSync, existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

// ----- Resolve port ---------------------------------------------------------
function portFilePath() {
  const base =
    process.env.SIMPLE_POST_DATA_DIR ||
    (process.platform === 'win32'
      ? join(process.env.APPDATA ?? join(homedir(), 'AppData', 'Roaming'), 'simple-post')
      : join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'simple-post'));
  return join(base, 'mcp.port');
}

let port = null;
for (let i = 2; i < process.argv.length; i++) {
  const arg = process.argv[i];
  if (arg.startsWith('--port=')) {
    port = Number(arg.slice('--port='.length));
  }
}
if (port === null) {
  const file = portFilePath();
  if (!existsSync(file)) {
    console.error(
      `mcp.port file not found at ${file}.\n` +
        `Launch the GUI first (it writes the file), or pass --port=PORT.`,
    );
    process.exit(2);
  }
  const rec = JSON.parse(readFileSync(file, 'utf8'));
  port = rec.port;
}

const url = `http://127.0.0.1:${port}/mcp`;
console.error(`[mcp-repl] POSTing to ${url} — type JSON-RPC frames, Ctrl-C to exit`);

// ----- Stream stdin → HTTP → stdout -----------------------------------------
const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', async (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: trimmed,
    });
    const text = await res.text();
    process.stdout.write(text + '\n');
  } catch (e) {
    console.error(`[mcp-repl] request failed: ${e.message}`);
  }
});
rl.on('close', () => process.exit(0));
