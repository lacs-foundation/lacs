#!/usr/bin/env node
'use strict';

// ---------------------------------------------------------------------------
// SysKnife setup — zero-dependency onboarding script
//
// Creates:
//   .mcp.json                                          MCP server config
//   .claude/hookify.require-sysknife-approval.local.md  Claude Code hook
//
// Usage:
//   npx sysknife-setup
// ---------------------------------------------------------------------------

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const readline = require('readline');

// ---------------------------------------------------------------------------
// Terminal helpers
// ---------------------------------------------------------------------------

const ESC = '\x1b[';
const G = `${ESC}32m`; // green
const Y = `${ESC}33m`; // yellow
const R = `${ESC}31m`; // red
const B = `${ESC}1m`;  // bold
const D = `${ESC}2m`;  // dim
const X = `${ESC}0m`;  // reset

function ok(msg)   { console.log(`  ${G}✓${X}  ${msg}`); }
function warn(msg) { console.log(`  ${Y}⚠${X}  ${msg}`); }
function step(msg) { console.log(`  ${D}→${X}  ${msg}`); }

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/** Locate a binary via `which` — uses execFile (no shell) for safety. */
function findBinary(name) {
  try {
    return execFileSync('which', [name], { stdio: ['pipe', 'pipe', 'pipe'] })
      .toString()
      .trim();
  } catch {
    return null;
  }
}

function ask(rl, question, defaultVal) {
  return new Promise((resolve) => {
    const suffix = defaultVal ? ` ${D}[${defaultVal}]${X}` : '';
    rl.question(`  ${question}${suffix}: `, (answer) => {
      resolve(answer.trim() || defaultVal || '');
    });
  });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const PROVIDERS = ['openai', 'anthropic', 'gemini', 'ollama'];

const MODEL_DEFAULTS = {
  openai:    'gpt-4.1',
  anthropic: 'claude-opus-4-6',
  gemini:    'gemini-2.5-pro',
  ollama:    'llama3.2:3b',
};

const API_KEY_VARS = {
  openai:    'OPENAI_API_KEY',
  anthropic: 'ANTHROPIC_API_KEY',
  gemini:    'GEMINI_API_KEY',
  ollama:    null,
};

// ---------------------------------------------------------------------------
// Hookify rule content
// ---------------------------------------------------------------------------

const HOOK_CONTENT = `---
name: require-sysknife-approval
enabled: true
event: prompt
pattern: .*
---

# sysknife execution rule (always active)

When using the sysknife MCP tools, you MUST follow this order:

1. Call \`sysknife_plan\` → present the plan to the user
2. **WAIT** for the user to explicitly approve
   (words like "yes", "do it", "execute", "go ahead", "approved")
3. Only then call \`sysknife_execute\`

**Never call \`sysknife_execute\` in the same turn as \`sysknife_plan\`.**
Always stop after showing the plan and wait for the user's response.
`;

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log();
  console.log(`${B}SysKnife Setup${X}`);
  console.log(`  Configures .mcp.json and the Claude Code approval hook.`);
  console.log();

  const rl = readline.createInterface({
    input:    process.stdin,
    output:   process.stdout,
    terminal: true,
  });

  // ── 1. Binary location ──────────────────────────────────────────────────

  const detected = findBinary('sysknife');
  if (detected) {
    ok(`Found sysknife at ${detected}`);
  } else {
    warn('sysknife not found in PATH — you can set the path manually below');
  }

  const binaryPath = await ask(
    rl,
    'Path to sysknife binary',
    detected || '/usr/local/bin/sysknife'
  );

  if (!fs.existsSync(binaryPath)) {
    warn(`${binaryPath} does not exist yet — update .mcp.json after installing`);
  }

  // ── 2. Daemon socket ────────────────────────────────────────────────────

  console.log();
  const socketPath = await ask(
    rl,
    'Daemon socket path',
    '/tmp/sysknife-daemon.sock'
  );

  // ── 3. LLM provider ─────────────────────────────────────────────────────

  console.log();
  const providerList = PROVIDERS.map((p, i) => (i === 0 ? `${B}${p}${X}` : p)).join(' / ');
  console.log(`  LLM providers: ${providerList}`);
  let provider = await ask(rl, 'LLM provider', 'openai');
  provider = provider.toLowerCase();

  if (!PROVIDERS.includes(provider)) {
    console.error(`\n  ${R}✗${X}  Unknown provider "${provider}". Choose: ${PROVIDERS.join(', ')}`);
    rl.close();
    process.exit(1);
  }

  // ── 4. API key ──────────────────────────────────────────────────────────

  const envVar = API_KEY_VARS[provider];
  let apiKey = '';

  if (envVar) {
    const existing = process.env[envVar];
    if (existing) {
      ok(`${envVar} already set in environment — will not embed in .mcp.json`);
    } else {
      console.log();
      console.log(`  ${Y}Note:${X} The key will be stored in .mcp.json in plain text.`);
      console.log(`  Leave blank to set ${envVar} in your shell profile instead.`);
      apiKey = await ask(rl, envVar, '');
    }
  }

  // ── 5. Model ─────────────────────────────────────────────────────────────

  console.log();
  const model = await ask(rl, 'Model name', MODEL_DEFAULTS[provider]);

  rl.close();

  // ── Write .mcp.json ──────────────────────────────────────────────────────

  console.log();

  const mcpEnv = {
    SYSKNIFE_SOCKET:       socketPath,
    SYSKNIFE_LLM_PROVIDER: provider,
    SYSKNIFE_LLM_MODEL:    model,
  };
  if (envVar && apiKey) {
    mcpEnv[envVar] = apiKey;
  }

  const mcpConfig = {
    mcpServers: {
      sysknife: {
        command: binaryPath,
        args:    ['mcp-server'],
        env:     mcpEnv,
      },
    },
  };

  fs.writeFileSync('.mcp.json', JSON.stringify(mcpConfig, null, 2) + '\n');
  ok('Created .mcp.json');

  // ── Write hookify rule ───────────────────────────────────────────────────

  if (!fs.existsSync('.claude')) {
    fs.mkdirSync('.claude', { recursive: true });
  }

  const hookPath = path.join('.claude', 'hookify.require-sysknife-approval.local.md');
  fs.writeFileSync(hookPath, HOOK_CONTENT);
  ok(`Created ${hookPath}`);

  // ── Gitignore advice ─────────────────────────────────────────────────────

  const gitignore = fs.existsSync('.gitignore')
    ? fs.readFileSync('.gitignore', 'utf8')
    : '';

  const noMcpEntry  = !gitignore.includes('.mcp.json');
  const noHookEntry = !gitignore.includes('*.local.md');
  if (noMcpEntry || noHookEntry) {
    console.log();
    warn('Consider adding these to .gitignore to avoid committing secrets:');
    if (noMcpEntry)  step('.mcp.json');
    if (noHookEntry) step('.claude/*.local.md');
  }

  // ── Next steps ───────────────────────────────────────────────────────────

  console.log();
  console.log(`${B}Next steps${X}`);
  console.log();

  if (!fs.existsSync(binaryPath)) {
    step('Build sysknife:  cargo build -p sysknife-cli --release');
  }

  step('Start the daemon:  sudo systemctl start sysknife-daemon');
  step('              or:  cargo run -p sysknife-daemon');
  step('Reload Claude Code:  type /reload-plugins in the Claude Code chat');

  if (envVar && !apiKey && !process.env[envVar]) {
    console.log();
    warn(`Set your API key before starting Claude Code:`);
    step(`export ${envVar}=your-key-here`);
  }

  console.log();
  ok('Setup complete');
  console.log();
}

main().catch((e) => {
  console.error(`\n  ${R}✗${X}  ${e.message}`);
  process.exit(1);
});
