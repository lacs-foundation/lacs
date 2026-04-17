#!/usr/bin/env node
'use strict';

// ---------------------------------------------------------------------------
// SysKnife setup — zero-dependency onboarding script
//
// Creates:
//   .mcp.json                                                  MCP server config
//   .claude/hookify.require-sysknife-approval.local.md         approval gate
//   .claude/hookify.sysknife-schema-fetch.local.md             schema-fetch reminder
//   .claude/hookify.sysknife-bash-guard.local.md               VM query guard
//
// Single VM:   npx sysknife-setup
// Multiple VMs: the wizard prompts "Add another VM?" and collects each target.
//   Each target becomes a separate mcpServers entry so Claude Code sees
//   independent, named tool sets (sysknife-web, sysknife-db, …).
// ---------------------------------------------------------------------------

const fs   = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const readline = require('readline');
const crypto   = require('crypto');

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
function hr()      { console.log(`  ${D}${'─'.repeat(52)}${X}`); }

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

/** Strip characters that are unsafe in MCP server key names. */
function sanitizeName(s) {
  return s.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '') || 'vm';
}

/** Generate a 32-byte hex token suitable for vsock auth. */
function generateToken() {
  return crypto.randomBytes(32).toString('hex');
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
//
// Always includes multi-VM fleet guidance even when a single target is
// configured — users may add targets manually later, and the rule stays valid.

const HOOK_APPROVAL = `---
name: require-sysknife-approval
enabled: true
event: prompt
pattern: .*
---

# SysKnife execution rules (always active)

## Single VM

When using the sysknife MCP tools, you MUST follow this order:

1. Call \`sysknife_plan\` → present the plan to the user
2. **WAIT** for the user to explicitly approve
   (words like "yes", "do it", "execute", "go ahead", "approved")
3. Only then call \`sysknife_execute\`

**Never call \`sysknife_execute\` in the same turn as \`sysknife_plan\`.**
Always stop after showing the plan and wait for the user's response.

## Multiple VMs (fleet operations)

When sysknife is configured with multiple targets (sysknife-web, sysknife-db, …):

1. Call \`sysknife_plan\` for **all** VMs that will be affected — before executing any
2. Present **all** plans together so the user can review the full scope of changes
3. **WAIT** for the user to explicitly approve all plans in a single response
4. Only then call \`sysknife_execute\` for each VM

**Never execute one VM while another VM's plan is still pending review.**
**Never skip showing a plan because it looks similar to another VM's plan.**
Each VM is independent — show each plan explicitly.
`;

const HOOK_SCHEMA_FETCH = `---
name: sysknife-schema-fetch
enabled: true
event: prompt
pattern: .*
---

# Deferred MCP tool schemas must be fetched before use

Sysknife MCP tools (\`sysknife_plan\`, \`sysknife_execute\`, \`sysknife_preview\`) are
registered as **deferred tools** — their full schemas are NOT loaded at session
start to save context.

**Before calling any sysknife tool you have not used yet this session:**
1. Call \`ToolSearch\` with the tool name (e.g. \`select:sysknife_plan\`) to fetch its schema.
2. Only then call the tool.

Skipping this step causes \`InputValidationError\` because the parameter schema is unknown.
`;

const HOOK_BASH_GUARD = `---
name: sysknife-bash-guard
enabled: true
event: bash
pattern: (?:date|hostname|uname|df|free|uptime|who|id|ps|top|systemctl|journalctl|ip\\s|ss\\s|netstat|lscpu|lsmem|cat\\s+/proc|dmidecode)
---

# Route VM system queries through sysknife — not local Bash

The command you are about to run queries system state.  If the user is asking
about their **QEMU/KVM guest VM**, this local Bash command returns host data —
not VM data.

**Before running local Bash for system queries:**
1. Check whether sysknife MCP tools are available (fetch deferred schemas via \`ToolSearch\`).
2. If sysknife is available, use \`sysknife_plan\` → approve → \`sysknife_execute\` instead.
3. Only run the local Bash command if sysknife is unavailable or the user explicitly asks for the local host.
`;

// ---------------------------------------------------------------------------
// Collect one VM target (socket + optional vsock token)
// ---------------------------------------------------------------------------

async function collectTarget(rl, idx) {
  console.log();
  console.log(`  ${B}── VM Target ${idx} ${'─'.repeat(40 - String(idx).length)}${X}`);
  console.log(`  ${D}Socket examples:${X}`);
  console.log(`    ${D}/run/sysknife/daemon.sock${X}   ${D}local daemon (systemd default)${X}`);
  console.log(`    ${D}/tmp/sysknife-vm.sock${X}        ${D}SSH tunnel to a VM${X}`);
  console.log(`    ${D}vsock://10:9734${X}              ${D}virtio-vsock (CID:port)${X}`);

  const socket = await ask(rl, 'Daemon socket', '/run/sysknife/daemon.sock');

  let token = '';
  if (socket.startsWith('vsock://')) {
    console.log();
    console.log(`  ${Y}vsock detected.${X} A pre-shared token is required.`);
    console.log(`  ${D}Leave blank to auto-generate one.${X}`);
    console.log(`  On the guest: ${D}echo "admin:<token>" | sudo tee /etc/sysknife/token${X}`);
    token = await ask(rl, 'SYSKNIFE_TOKEN (hex)', '');
    if (!token) {
      token = generateToken();
      ok(`Generated vsock auth token: ${token}`);
      warn('Copy this token to the guest VM at /etc/sysknife/token');
    }
  }

  return { socket, token };
}

// ---------------------------------------------------------------------------
// Next-step hint for a single target
// ---------------------------------------------------------------------------

function targetNextStep(target) {
  const { socket, name } = target;
  const label = name ? `${name} (${socket})` : socket;

  if (socket.startsWith('vsock://')) {
    step(`Start daemon in ${label} guest:  sudo systemctl start sysknife-daemon`);
  } else if (socket !== '/run/sysknife/daemon.sock' && socket !== '/tmp/sysknife-daemon.sock') {
    // Likely an SSH tunnel socket — remind user to open the tunnel
    step(`Open SSH tunnel for ${label}:  ssh -fN -L ${socket}:/run/sysknife/daemon.sock <user>@<host>`);
    step(`Then start the daemon in the guest:  sudo systemctl start sysknife-daemon`);
  } else {
    step('Start the daemon:  sudo systemctl start sysknife-daemon');
    step('              or:  cargo run -p sysknife-daemon');
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log();
  console.log(`${B}SysKnife Setup${X}`);
  console.log(`  Configures .mcp.json and Claude Code hooks.`);
  console.log(`  Supports single and multi-VM (fleet) configurations.`);
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

  // ── 2. LLM provider ─────────────────────────────────────────────────────
  // Collected once and shared across all targets — each MCP server process
  // uses the same model, only the daemon socket differs.

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

  // ── 3. API key ──────────────────────────────────────────────────────────

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

  // ── 4. Model ─────────────────────────────────────────────────────────────

  console.log();
  const model = await ask(rl, 'Model name', MODEL_DEFAULTS[provider]);

  // ── 5. VM targets (loop) ─────────────────────────────────────────────────

  const targets = [];
  let addingMore = true;

  while (addingMore) {
    const target = await collectTarget(rl, targets.length + 1);
    targets.push(target);

    console.log();
    const answer = await ask(rl, 'Add another VM?', 'N');
    addingMore = answer.toLowerCase().startsWith('y');
  }

  // ── 6. Names for multi-VM (only when >1 target) ──────────────────────────
  //
  // Single target: mcpServers key stays "sysknife" — fully backward-compatible.
  // Multiple targets: user picks a short label for each; keys become
  // "sysknife-<name>".  Names are collected after all targets are entered so
  // the happy-path (single VM) gets no extra prompts.

  if (targets.length > 1) {
    console.log();
    hr();
    console.log(`  ${B}Name your targets${X}`);
    console.log(`  ${D}Names become MCP server IDs: sysknife-<name>${X}`);
    console.log(`  ${D}Claude will see sysknife-<name>__sysknife_plan, etc.${X}`);
    console.log();

    for (let i = 0; i < targets.length; i++) {
      const defaultName = `vm${i + 1}`;
      const raw = await ask(rl, `Name for ${targets[i].socket}`, defaultName);
      targets[i].name = sanitizeName(raw);
    }

    // Deduplicate: if two targets got the same sanitized name, suffix with index
    const seen = new Map();
    for (const t of targets) {
      const count = seen.get(t.name) ?? 0;
      if (count > 0) { t.name = `${t.name}-${count + 1}`; }
      seen.set(t.name, count + 1);
    }
  }

  rl.close();

  // ── Build mcpServers entries ─────────────────────────────────────────────

  const sharedEnv = {
    SYSKNIFE_LLM_PROVIDER: provider,
    SYSKNIFE_LLM_MODEL:    model,
  };
  if (envVar && apiKey) {
    sharedEnv[envVar] = apiKey;
  }

  function makeServer(target) {
    const env = { SYSKNIFE_SOCKET: target.socket, ...sharedEnv };
    if (target.token) { env['SYSKNIFE_TOKEN'] = target.token; }
    return { command: binaryPath, args: ['mcp-server'], env };
  }

  const mcpServers = {};
  if (targets.length === 1) {
    // Single target — backward-compatible key "sysknife"
    mcpServers['sysknife'] = makeServer(targets[0]);
  } else {
    for (const t of targets) {
      mcpServers[`sysknife-${t.name}`] = makeServer(t);
    }
  }

  // ── Write .mcp.json ──────────────────────────────────────────────────────

  console.log();

  const mcpConfig = { mcpServers };
  fs.writeFileSync('.mcp.json', JSON.stringify(mcpConfig, null, 2) + '\n');

  const serverKeys = Object.keys(mcpServers);
  const targetSummary = targets.length === 1
    ? 'sysknife'
    : serverKeys.join(', ');
  ok(`Created .mcp.json  (${targets.length} target${targets.length > 1 ? 's' : ''}: ${targetSummary})`);

  // ── Write hookify rules ──────────────────────────────────────────────────

  if (!fs.existsSync('.claude')) {
    fs.mkdirSync('.claude', { recursive: true });
  }

  const rules = [
    { file: 'hookify.require-sysknife-approval.local.md', content: HOOK_APPROVAL },
    { file: 'hookify.sysknife-schema-fetch.local.md',     content: HOOK_SCHEMA_FETCH },
    { file: 'hookify.sysknife-bash-guard.local.md',       content: HOOK_BASH_GUARD },
  ];

  for (const { file, content } of rules) {
    const hookPath = path.join('.claude', file);
    fs.writeFileSync(hookPath, content);
    ok(`Created ${hookPath}`);
  }

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
    console.log();
  }

  for (const t of targets) {
    targetNextStep(t);
  }

  console.log();
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
