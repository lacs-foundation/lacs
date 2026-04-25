//! External audit log forwarding (RFC 5424 syslog over UDP).
//!
//! After each `transactions` row is recorded, the daemon hands a structured
//! `AuditEvent` to a per-sink forwarder task over a bounded
//! [`tokio::sync::mpsc::channel`]. The forwarder formats the event per the
//! configured wire protocol and emits it. **Forwarding is fire-and-forget**:
//! channel send failures (`try_send` returning `Full`/`Closed`) increment a
//! drop counter and emit an `eprintln!` warning after `DROP_WARN_THRESHOLD`
//! consecutive drops; the audit-log INSERT path itself never awaits or fails.
//!
//! ## Wire protocols
//!
//! Phase 1 ships **RFC 5424 syslog over UDP**, the de-facto on-host
//! log-forwarding format. Direct ingestion works with Splunk, Elastic,
//! IBM QRadar, and rsyslog; vendors that require a forwarder agent
//! (Microsoft Sentinel via the Azure Monitor Agent on a Linux VM,
//! Datadog/Chronicle via their own collectors) consume the same stream
//! through that agent. CEF and NDJSON-over-TCP are designed-for in
//! [`AuditSinkSpec`] and arrive in follow-up PRs.
//!
//! ## Reliability vs durability
//!
//! - **Local audit-log INSERT** (with hash chain, #149) is the durable record.
//! - **External forwarding** is best-effort. A SIEM outage, a routing flap, or
//!   a misconfigured collector must NEVER block daemon execution.
//! - We do not retry-with-backoff in-process: the SIEM is the long-lived
//!   accumulator; if it's down, the local hash-chained log will catch up the
//!   operator on the next ingest. (A future "tail watermark + replay" Phase 2
//!   bridge can reconcile gaps from the local log.)

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sysknife_types::RiskLevel;
use tokio::sync::mpsc;

/// Bounded queue depth between daemon and forwarder. Sized for ~5 minutes of
/// audit events at the daemon's sustainable record rate (well below SIEM
/// ingest); a sustained burst that overflows is a SIEM outage and we drop
/// (with counter + WARN) rather than back-pressure the audit-log writer.
pub const FORWARDER_QUEUE_DEPTH: usize = 4096;

/// Emit a single WARN to stderr after this many consecutive `try_send` drops.
/// Counter resets on a successful send.
pub const DROP_WARN_THRESHOLD: u64 = 8;

/// Configuration for one forwarding sink.
#[derive(Clone, Debug)]
pub enum AuditSinkSpec {
    /// RFC 5424 syslog over UDP. Sends each event as a single datagram.
    SyslogUdp {
        /// Host:port of the receiver, e.g. `"siem.internal:514"`.
        host: SocketAddr,
        /// Syslog facility (default `1` = user-level messages).
        facility: u8,
    },
}

/// One audit event handed to the forwarder. Mirrors the chain content
/// captured at INSERT time so SIEM rules can correlate by `transaction_id`
/// and `request_hash` against the local hash-chained log.
#[derive(Clone, Debug)]
pub struct AuditEvent {
    pub seq: u64,
    pub transaction_id: String,
    pub action_name: String,
    pub risk_level: RiskLevel,
    pub summary: String,
    pub approval_id: Option<String>,
    pub created_at: String,
    pub chain_hash: String,
    pub key_id: String,
    pub caller_role: Option<String>,
}

/// A handle the daemon writes to. Cheap to clone (Arc-wrapped sender +
/// counter). Returns immediately on `submit`.
#[derive(Clone)]
pub struct AuditForwarder {
    sender: mpsc::Sender<AuditEvent>,
    drops: Arc<AtomicU64>,
}

impl std::fmt::Debug for AuditForwarder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditForwarder")
            .field("queue_capacity", &self.sender.capacity())
            .field("queue_max", &self.sender.max_capacity())
            .field("drops", &self.drops.load(Ordering::Relaxed))
            .finish()
    }
}

impl AuditForwarder {
    /// Submit an event for forwarding. Never blocks. Drops the event if the
    /// channel is full or closed; consecutive drops emit a WARN after
    /// [`DROP_WARN_THRESHOLD`].
    pub fn submit(&self, event: AuditEvent) {
        match self.sender.try_send(event) {
            Ok(()) => {
                // Reset drop counter on a successful submit so the next
                // outage emits a fresh WARN.
                self.drops.store(0, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                let prev = self.drops.fetch_add(1, Ordering::Relaxed);
                if (prev + 1) == DROP_WARN_THRESHOLD {
                    eprintln!(
                        "[sysknife-daemon] audit-forward: queue full, dropping events \
                         (>= {DROP_WARN_THRESHOLD} consecutive); is the SIEM reachable?"
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The forwarder task has shut down. One-time WARN to stderr;
                // subsequent submits silently no-op.
                let prev = self.drops.fetch_add(1, Ordering::Relaxed);
                if prev == 0 {
                    eprintln!(
                        "[sysknife-daemon] audit-forward: forwarder task is gone; \
                         further submits will be dropped silently"
                    );
                }
            }
        }
    }

    /// Total events dropped since the last successful submit. Test-only.
    #[cfg(test)]
    pub fn drop_count(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }
}

/// Spawn a forwarder task that consumes events from `rx` and sends them to
/// `spec`. Returns the [`AuditForwarder`] handle the daemon writes to.
///
/// On task exit (channel closed by all senders dropping), the task returns
/// silently. There is no graceful drain — pending events in the channel are
/// dropped at shutdown. Audit durability is guaranteed by the local
/// hash-chained log, not by the forwarder.
pub fn spawn(spec: AuditSinkSpec) -> AuditForwarder {
    let (tx, rx) = mpsc::channel(FORWARDER_QUEUE_DEPTH);
    let drops = Arc::new(AtomicU64::new(0));
    tokio::spawn(forwarder_task(spec, rx));
    AuditForwarder { sender: tx, drops }
}

async fn forwarder_task(spec: AuditSinkSpec, mut rx: mpsc::Receiver<AuditEvent>) {
    match spec {
        AuditSinkSpec::SyslogUdp { host, facility } => {
            let mut socket = open_udp(host).await;
            // Exponential backoff on consecutive bind failures (F11).
            // 1s → 2s → 4s → … capped at 60s. Reset to 1s on first success.
            let mut backoff_secs: u64 = 1;
            while let Some(event) = rx.recv().await {
                let frame = format_rfc5424(&event, facility);
                if let Some(s) = &socket {
                    if let Err(e) = s.send_to(frame.as_bytes(), host).await {
                        eprintln!(
                            "[sysknife-daemon] audit-forward: UDP send to {host} failed: {e} \
                             — dropping socket and reopening"
                        );
                        socket = None;
                    }
                }
                if socket.is_none() {
                    socket = open_udp(host).await;
                    if socket.is_none() {
                        // Bind failure: sleep with exponential backoff before
                        // we attempt again on the next event. Without this,
                        // a misconfigured host with a packed channel would
                        // pin a CPU at 100% on `eprintln!` syscalls.
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        backoff_secs = (backoff_secs.saturating_mul(2)).min(60);
                    } else {
                        backoff_secs = 1;
                    }
                }
            }
        }
    }
}

/// Bind a fresh ephemeral UDP socket whose address family matches `host`.
///
/// IPv4 host → bind on `0.0.0.0:0`; IPv6 host → bind on `[::]:0`. Without
/// this, an operator with an IPv6-only SIEM (e.g. Sentinel at
/// `[2001:db8::5]:514`) would never receive any events because every send
/// would fail with `AddrNotAvailable` (F8).
///
/// Returns `None` on bind failure — caller retries with backoff.
async fn open_udp(host: SocketAddr) -> Option<tokio::net::UdpSocket> {
    let bind_addr = if host.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    match tokio::net::UdpSocket::bind(bind_addr).await {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[sysknife-daemon] audit-forward: UDP bind on {bind_addr} failed: {e}");
            None
        }
    }
}

/// Format `event` as a single RFC 5424 syslog message.
///
/// Layout (one line, no trailing newline — UDP datagrams don't need one):
/// ```text
/// <PRI>1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID [SD@32473 ...] MSG
/// ```
///
/// We hold to the spec's printable-USASCII rule for the structured-data
/// (SD) section by escaping `]`, `"`, and `\` per §6.3.3.
pub fn format_rfc5424(event: &AuditEvent, facility: u8) -> String {
    // Severity 5 = NOTICE for audit events. PRI = facility * 8 + severity.
    let severity = 5u8;
    let pri = (facility as u32) * 8 + severity as u32;

    let hostname = read_hostname();
    let app_name = "sysknife-daemon";
    let procid = std::process::id();
    let msgid = "AUDIT";

    let risk = match event.risk_level {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    };

    // RFC 5424 §6.3 — structured data. Use a private enterprise number 32473
    // (RFC 5612 reserved test PEN) for the SD-ID; production deployments
    // should change this to their org's PEN.
    let sd = format!(
        "[sysknife@32473 \
         seq=\"{}\" \
         tx=\"{}\" \
         action=\"{}\" \
         risk=\"{}\" \
         approval=\"{}\" \
         role=\"{}\" \
         chain_hash=\"{}\" \
         key_id=\"{}\"]",
        event.seq,
        sd_escape(&event.transaction_id),
        sd_escape(&event.action_name),
        risk,
        sd_escape(event.approval_id.as_deref().unwrap_or("")),
        sd_escape(event.caller_role.as_deref().unwrap_or("")),
        sd_escape(&event.chain_hash),
        sd_escape(&event.key_id),
    );

    let msg = format!("[{}] {}", event.action_name, event.summary);

    format!(
        "<{pri}>1 {ts} {host} {app} {pid} {msgid} {sd} {msg}",
        ts = event.created_at,
        host = hostname,
        app = app_name,
        pid = procid,
    )
}

/// Escape a value for inclusion inside a RFC 5424 SD-PARAM-VALUE per §6.3.3.
///
/// RFC 5424 §6 names PRINTUSASCII (`0x21..=0x7E`) as the preferred SD form;
/// UTF-8 is technically allowed but every strict SIEM ingest pipeline we
/// have surveyed (Splunk in strict mode, IBM QRadar, Microsoft Sentinel via
/// the Azure Monitor Agent) rejects non-ASCII bytes inside SD-VALUE. We
/// therefore drop **every** byte outside the printable-ASCII range and
/// escape the three characters
/// `]`, `"`, `\` per §6.3.3.
///
/// This also covers DEL (`0x7F`) and C1 controls (`0x80..=0x9F`).
fn sd_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ']' => out.push_str("\\]"),
            c if (0x20..=0x7E).contains(&(c as u32)) => out.push(c),
            // Drop everything else (C0, DEL, C1, all non-ASCII Unicode).
            _ => {}
        }
    }
    out
}

/// Read the kernel hostname and validate it against RFC 5424 §6.2.4.
///
/// HOSTNAME must be a single token: 1..=255 bytes, all `PRINTUSASCII`
/// (`0x21..=0x7E`), no embedded whitespace. Anything else (empty,
/// embedded space, non-ASCII, control char) returns the NILVALUE `"-"`
/// so the datagram remains parseable. Without this, a sysctl-set
/// hostname containing a space (e.g. `"edge node"`) would corrupt every
/// emitted frame and silently drop in strict SIEMs.
fn read_hostname() -> String {
    let raw = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if raw.is_empty() || raw.len() > 255 || !raw.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
        return "-".to_string();
    }
    raw
}

/// Test-only hostname validator that accepts an externally-provided string,
/// applies the same RFC 5424 §6.2.4 rule, and returns either the input or
/// `"-"`. Lets tests cover the validation path without mutating
/// `/proc/sys/kernel/hostname`.
#[cfg(test)]
fn validate_hostname_for_test(raw: &str) -> String {
    if raw.is_empty() || raw.len() > 255 || !raw.bytes().all(|b| (0x21..=0x7E).contains(&b)) {
        return "-".to_string();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> AuditEvent {
        AuditEvent {
            seq: 42,
            transaction_id: "tx-abc".to_string(),
            action_name: "InstallFlatpak".to_string(),
            risk_level: RiskLevel::Medium,
            summary: "Install Firefox".to_string(),
            approval_id: Some("appr-xyz".to_string()),
            created_at: "2026-04-25T08:30:00Z".to_string(),
            chain_hash: "deadbeef".to_string(),
            key_id: "v1".to_string(),
            caller_role: Some("Dev".to_string()),
        }
    }

    // ── Hostname validation (red-team finding F2) ────────────────────────

    #[test]
    fn hostname_with_space_is_rejected() {
        assert_eq!(validate_hostname_for_test("edge node 7"), "-");
    }

    #[test]
    fn hostname_with_control_byte_is_rejected() {
        assert_eq!(validate_hostname_for_test("edge\x01node"), "-");
    }

    #[test]
    fn hostname_with_non_ascii_is_rejected() {
        assert_eq!(validate_hostname_for_test("ëdge"), "-");
    }

    #[test]
    fn empty_hostname_falls_back_to_nilvalue() {
        assert_eq!(validate_hostname_for_test(""), "-");
    }

    #[test]
    fn legitimate_hostname_passes_through() {
        assert_eq!(
            validate_hostname_for_test("edge-node-7.example.com"),
            "edge-node-7.example.com"
        );
    }

    #[test]
    fn hostname_over_255_bytes_is_rejected() {
        let long = "a".repeat(256);
        assert_eq!(validate_hostname_for_test(&long), "-");
    }

    // ── SD-VALUE strict ASCII (F3) ───────────────────────────────────────

    #[test]
    fn sd_escape_strips_del_and_c1_controls() {
        let raw = "before\x7Fafter\u{0085}";
        assert_eq!(sd_escape(raw), "beforeafter");
    }

    #[test]
    fn sd_escape_strips_non_ascii_to_avoid_strict_siem_rejection() {
        // Splunk/QRadar in strict mode reject non-ASCII inside SD-VALUE.
        let raw = "café";
        let escaped = sd_escape(raw);
        assert!(escaped.bytes().all(|b| (0x20..=0x7E).contains(&b)));
    }

    // ── RFC 5424 framing ──────────────────────────────────────────────────

    #[test]
    fn rfc5424_starts_with_pri_and_version() {
        let frame = format_rfc5424(&sample_event(), 1);
        // facility=1, severity=5 → PRI = 13. Version = 1.
        assert!(frame.starts_with("<13>1 "));
    }

    #[test]
    fn rfc5424_contains_sd_with_chain_hash_and_seq() {
        let frame = format_rfc5424(&sample_event(), 1);
        assert!(frame.contains("[sysknife@32473"));
        assert!(frame.contains("seq=\"42\""));
        assert!(frame.contains("chain_hash=\"deadbeef\""));
        assert!(frame.contains("key_id=\"v1\""));
        assert!(frame.contains("risk=\"medium\""));
        assert!(frame.contains("role=\"Dev\""));
    }

    #[test]
    fn rfc5424_message_section_contains_summary_and_action_tag() {
        let frame = format_rfc5424(&sample_event(), 1);
        assert!(frame.ends_with("[InstallFlatpak] Install Firefox"));
    }

    #[test]
    fn sd_escape_handles_dquote_backslash_bracket() {
        let raw = r#"name with "quotes", a \ backslash, and ] bracket"#;
        let escaped = sd_escape(raw);
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\\\"));
        assert!(escaped.contains("\\]"));
    }

    #[test]
    fn sd_escape_strips_control_characters() {
        // Control bytes must not appear inside SD-VALUE.
        let raw = "before\x01\x02\x1fafter";
        let escaped = sd_escape(raw);
        assert_eq!(escaped, "beforeafter");
    }

    #[test]
    fn rfc5424_missing_approval_renders_empty_string() {
        let mut e = sample_event();
        e.approval_id = None;
        let frame = format_rfc5424(&e, 1);
        assert!(frame.contains("approval=\"\""));
    }

    #[test]
    fn rfc5424_caller_role_with_quote_is_escaped() {
        let mut e = sample_event();
        e.caller_role = Some(r#"Dev"name"#.to_string());
        let frame = format_rfc5424(&e, 1);
        assert!(frame.contains("role=\"Dev\\\"name\""));
    }

    #[test]
    fn rfc5424_facility_changes_pri() {
        // facility=23 (local7), severity=5 → PRI = 189.
        let frame = format_rfc5424(&sample_event(), 23);
        assert!(frame.starts_with("<189>1 "));
    }

    // ── AuditForwarder behaviour ─────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn forwarder_drops_on_closed_channel() {
        let (tx, rx) = mpsc::channel::<AuditEvent>(1);
        // Drop the receiver to simulate a forwarder task that has exited.
        drop(rx);
        let forwarder = AuditForwarder {
            sender: tx,
            drops: Arc::new(AtomicU64::new(0)),
        };
        forwarder.submit(sample_event());
        assert_eq!(forwarder.drop_count(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forwarder_drops_when_full_and_warns_at_threshold() {
        // Build a channel of capacity 1 so the second submit overflows.
        let (tx, _rx) = mpsc::channel::<AuditEvent>(1);
        let forwarder = AuditForwarder {
            sender: tx,
            drops: Arc::new(AtomicU64::new(0)),
        };

        // Fill the channel.
        forwarder.submit(sample_event()); // queued — drop_count stays 0
        assert_eq!(forwarder.drop_count(), 0);

        // Now overflow DROP_WARN_THRESHOLD times.
        for _ in 0..DROP_WARN_THRESHOLD {
            forwarder.submit(sample_event());
        }
        assert_eq!(forwarder.drop_count(), DROP_WARN_THRESHOLD);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forwarder_resets_drop_counter_after_successful_submit() {
        let (tx, mut rx) = mpsc::channel::<AuditEvent>(1);
        let forwarder = AuditForwarder {
            sender: tx,
            drops: Arc::new(AtomicU64::new(0)),
        };
        // First submit lands; second drops (full).
        forwarder.submit(sample_event());
        forwarder.submit(sample_event());
        assert_eq!(forwarder.drop_count(), 1);
        // Drain — frees a slot.
        let _ = rx.recv().await;
        // Next submit succeeds and resets the counter.
        forwarder.submit(sample_event());
        assert_eq!(forwarder.drop_count(), 0);
    }

    // ── End-to-end: spawn() actually emits over a UDP loopback ───────────

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_sends_udp_datagrams_to_listener() {
        // Bind a loopback listener first to learn the assigned port.
        let listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let host: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        let forwarder = spawn(AuditSinkSpec::SyslogUdp { host, facility: 1 });
        forwarder.submit(sample_event());

        // Receive with a generous timeout.
        let mut buf = [0u8; 2048];
        let len = tokio::time::timeout(Duration::from_secs(2), listener.recv(&mut buf))
            .await
            .expect("UDP recv timed out — forwarder did not emit")
            .expect("UDP recv succeeded");
        let frame = std::str::from_utf8(&buf[..len]).expect("frame is UTF-8");
        assert!(frame.starts_with("<13>1 "), "unexpected frame: {frame}");
        assert!(frame.contains("seq=\"42\""));
        assert!(frame.contains("[InstallFlatpak] Install Firefox"));
    }
}
