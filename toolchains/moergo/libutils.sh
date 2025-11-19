#!/usr/bin/env bash
# libutil.sh - Common functions library

# if [[ -n "$_LIBUTIL_SOURCED" ]]; then
#   return 0
# fi
# _LIBUTIL_SOURCED=1

# Set the logging level (DEBUG=0, INFO=1, WARN=2, ERROR=3)
: "${LOG_LEVEL:=1}" # Default to INFO level

log() {
  local level="$1"
  local level_num
  shift

  # Convert level name to number
  case "$level" in
  "DEBUG") level_num=0 ;;
  "INFO") level_num=1 ;;
  "WARN") level_num=2 ;;
  "ERROR") level_num=3 ;;
  *) level_num=1 ;; # Default to INFO
  esac

  # Only log if the message level is >= current LOG_LEVEL
  if [ "$level_num" -ge "$LOG_LEVEL" ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] [${level}] $*" >&2
  fi
}

log_info() { log "INFO" "$@"; }
log_warn() { log "WARN" "$@"; }
log_error() { log "ERROR" "$@"; }
log_debug() { log "DEBUG" "$@"; }
