# LACS Makefile — build, install, and uninstall the daemon and shell.
#
# Typical usage (as root or via sudo):
#   make build
#   sudo make install
#   sudo make uninstall
#
# PREFIX can be overridden: sudo make install PREFIX=/opt/lacs

PREFIX      ?= /usr/local
BINDIR      := $(PREFIX)/bin
SYSUSERS    := /usr/lib/sysusers.d
TMPFILES    := /usr/lib/tmpfiles.d
SYSTEMD     := /usr/lib/systemd/system
POLKIT      := /usr/share/polkit-1/rules.d
SUDOERS     := /etc/sudoers.d

CARGO_BUILD_FLAGS ?= --release --locked

.PHONY: build install uninstall daemon-install daemon-uninstall check

## ── Build ────────────────────────────────────────────────────────────────────

build:
	cargo build $(CARGO_BUILD_FLAGS) -p lacs-daemon
	@echo "Build complete. Binary: target/release/lacs-daemon"

## ── Install ──────────────────────────────────────────────────────────────────

install: daemon-install
	@echo ""
	@echo "LACS daemon installed. Run 'sudo systemctl enable --now lacs-daemon' to start."

daemon-install: build
	install -Dm 755 target/release/lacs-daemon $(BINDIR)/lacs-daemon

	# System user and group (idempotent via systemd-sysusers).
	install -Dm 644 packaging/lacs-sysusers.conf $(SYSUSERS)/lacs.conf
	systemd-sysusers $(SYSUSERS)/lacs.conf

	# Runtime and state directories (idempotent via systemd-tmpfiles).
	install -Dm 644 packaging/lacs-tmpfiles.conf $(TMPFILES)/lacs.conf
	systemd-tmpfiles --create $(TMPFILES)/lacs.conf

	# systemd unit.
	install -Dm 644 packaging/lacs-daemon.service $(SYSTEMD)/lacs-daemon.service
	systemctl daemon-reload

	# polkit rules.
	install -Dm 644 packaging/50-lacs.rules $(POLKIT)/50-lacs.rules

	# sudoers fragment (visudo validates before install).
	visudo -cf packaging/lacs-sudoers
	install -Dm 440 packaging/lacs-sudoers $(SUDOERS)/lacs

## ── Uninstall ────────────────────────────────────────────────────────────────

uninstall: daemon-uninstall

daemon-uninstall:
	-systemctl disable --now lacs-daemon 2>/dev/null || true
	rm -f $(BINDIR)/lacs-daemon
	rm -f $(SYSTEMD)/lacs-daemon.service
	systemctl daemon-reload
	rm -f $(POLKIT)/50-lacs.rules
	rm -f $(SUDOERS)/lacs
	rm -f $(SYSUSERS)/lacs.conf
	rm -f $(TMPFILES)/lacs.conf
	@echo "Daemon uninstalled. User 'lacs' and /var/lib/lacs data were NOT removed."
	@echo "To remove them manually: userdel lacs && rm -rf /var/lib/lacs /run/lacs"

## ── Dev checks ───────────────────────────────────────────────────────────────

check:
	cargo test --workspace --locked
	cargo clippy --workspace --locked -- -D warnings
