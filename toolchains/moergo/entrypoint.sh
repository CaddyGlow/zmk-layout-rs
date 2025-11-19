#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
source "$SCRIPT_DIR/libutils.sh"

: "${LOG_LEVEL}:=1" # Default to INFO level
: "${WORKSPACE_DIR:=/workspace}"
: "${ZMK_DIR:=/src}"
: "${PUID:=}"
: "${PGID:=}"

# Set desired umask
: "${UMASK:=0022}"
umask "$UMASK"

if [ "$(id -u)" = "0" ] && [ -n "$PUID" ] && [ -n "$PGID" ]; then
  # Handle PUID/PGID mapping
  log_info "Using PUID:PGID $PUID:$PGID"
  if command -v gosu >/dev/null 2>&1; then
    log_info "Using gosu for UID/GID mapping"
    chown -R "$PUID:$PGID" "$ZMK_DIR" /zmk
    if ! getent group $PGID >/dev/null 2>&1; then
      groupadd -g $PGID usergroup
    fi

    if ! id -u $PUID >/dev/null 2>&1; then
      useradd -m -u $PUID -g $PGID -s /bin/bash user
    fi
    exec gosu $PUID:$PGID "$@"
  else
    # Cleanup function
    cleanup() {
      log_info "CLEANUP: Running cleanup ..."
      log_info "CLEANUP: Attempting chmod ${WORKSPACE_DIR}"
      # poor man find -tf -exec chmod 644 {} \; ${WORKSPACE_DIR} && find -td -exec chmod 755 {} \; ${WORKSPACE_DIR}
      chmod 644 ${WORKSPACE_DIR}/* || log_warn "CLEANUP: chmod failed"
      chmod 755 ${WORKSPACE_DIR}/*/ || log_warn "CLEANUP: chmod failed"
      chmod 644 ${WORKSPACE_DIR}/*/* || log_warn "CLEANUP: chmod failed"
      chmod 755 ${WORKSPACE_DIR}/*/*/ || log_warn "CLEANUP: chmod failed"
      if [ -n "${PUID:-}" ] && [ -n "${PGID:-}" ]; then
        log_info "CLEANUP: Attempting chown -R "$PUID:$PGID" ${WORKSPACE_DIR}"
        chown -R "$PUID:$PGID" ${WORKSPACE_DIR} || log_warn "CLEANUP: chown failed"
        chown -R "$PUID:$PGID" ${WORKSPACE_DIR} || log_warn "CLEANUP: chown failed"
      fi
    }

    # Handle Docker signals properly
    trap 'log_debug "TRAP: EXIT"; cleanup' EXIT
    trap 'log_warn "TRAP: INT (Ctrl+C)"; cleanup; exit 130' INT
    trap 'log_warn "TRAP: TERM (Docker stop)"; cleanup; exit 143' TERM

    log_debug "Setting up signal traps..."
    log_debug "Running command: $*"

    # Execute the command and wait for it
    "$@" &
    child_pid=$!
    wait $child_pid
    exit_code=$?

    log_debug "Command completed with exit code: $exit_code"
    exit $exit_code
  fi
fi

exec "$@"
