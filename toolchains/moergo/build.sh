#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
source "$SCRIPT_DIR/libutils.sh"

# Generate build ID
generate_build_id() {
  # local config_sum=$(cat "${KEYMAP}" "${KCONFIG}" | sha256sum | cut -d' ' -f1 | cut -c1-8)
  local config_sum=$({
    echo "$BOARD_NAME"
    cat "${KEYMAP}" "${KCONFIG}"
  } | sha256sum | cut -d' ' -f1 | cut -c1-8)
  local git_hash=$(cd "${ZMK_DIR}" && git rev-parse --short HEAD)
  local timestamp=$(date +%Y%m%d%H%M%S)
  local branch_tag=$(echo ${BRANCH} | tr '/' '-' | cut -c1-20)

  echo "${BOARD_NAME}-${config_sum}${git_hash}-${branch_tag}-${timestamp}"
}

# Default parameters
: "${WORKSPACE_DIR:=/workspace}"
: "${ZMK_DIR:=/src}"
: "${BRANCH:=v25.05}"
: "${REPO:=https://github.com/moergo-sc/zmk}"
: "${ARTIFACTS_DIR:=${WORKSPACE_DIR}/artifacts}"
: "${BOARD_NAME:=glove80}"
: "${KEYMAP:=${WORKSPACE_DIR}/config/${BOARD_NAME}.keymap}"
: "${KCONFIG:=${WORKSPACE_DIR}/config/${BOARD_NAME}.conf}"
: "${LOG_LEVEL:=1}" # Default to INFO level
: "${TMPDIR:=/tmp}"
: "${NIX_ARGS:=--keep-failed}"
: "${BUILD_ID:=$(generate_build_id)}"
: "${BUILD_LOG:=${ARTIFACTS_DIR}/build.log}"

mkdir -p "${ARTIFACTS_DIR}"

dest_lh_dir="${ARTIFACTS_DIR}/${BOARD_NAME}_lh-zmk/"
dest_rh_dir="${ARTIFACTS_DIR}/${BOARD_NAME}_rh-zmk/"

# Create directories for each hand
mkdir -p dest_lh_dir
mkdir -p dest_rh_dir

# Collect generated artifacts from the build
# Used in case of failure to collect the
# generated DTS and devicetree header needed to
# understand errors
collect_generated_artifacts() {
  log_info "Collecting generated artifacts..."

  # Helper function to copy file if it exists
  copy_if_exists() {
    local src="$1"
    local dest="$2"
    local description="$3"

    if [ -f "$src" ]; then
      cp -f "$src" "$dest"
      log_info "Copied $description"
    fi
  }

  # Copy common artifacts
  cp "$KEYMAP" "${ARTIFACTS_DIR}/"
  cp "$KCONFIG" "${ARTIFACTS_DIR}/"

  # Define side-specific configuration
  local sides=("rh" "lh")
  local descriptions=("RH" "LF") # Note: keeping original LF for left hand
  local dest_dirs=("$dest_rh_dir" "$dest_lh_dir")

  # Process each side
  for i in "${!sides[@]}"; do
    local side="${sides[$i]}"
    local desc="${descriptions[$i]}"
    local dest_dir="${dest_dirs[$i]}"
    local base_path="${TMPDIR}/nix-build-zmk_glove80_${side}.drv-0/source/app/build/zephyr"

    # Copy artifacts for this side
    copy_if_exists "${base_path}/zephyr.dts" "$dest_dir" "${desc} DTS"
    copy_if_exists "${base_path}/zephyr.dts.pre" "$dest_dir" "${desc} DTS"
    copy_if_exists "${base_path}/include/generated/devicetree_generated.h" "$dest_dir" "${desc} devicetree header"
  done

  log_info "Artifact collection completed"
}

# Main function
main() {
  log_info "Starting Glove80 firmware build"
  log_info "Branch: $BRANCH"
  log_info "Repository: $REPO"

  # Setup repository
  cd ${ZMK_DIR}
  git fetch origin
  git checkout -q --detach "$BRANCH"

  GIT_COMMIT=$(git rev-parse HEAD)
  log_info "Git commit: $GIT_COMMIT"

  # Build firmware
  log_info "Building Glove80 firmware..."
  cd ${WORKSPACE_DIR}

  (
    log_info "Running nix-build with args: keymap=$KEYMAP, kconfig=$KCONFIG, buildId=$BUILD_ID"
    log_info "Environment:"
    log_info "$(env)"

    nix-build "${WORKSPACE_DIR}/config/default.nix" --argstr keymap "$KEYMAP" --argstr kconfig "$KCONFIG" --argstr buildId "$BUILD_ID" "$NIX_ARGS" -o result 2>&1 | tee -a "$BUILD_LOG"
  ) || {
    log_error "Build failed, see log at $BUILD_LOG"

    collect_generated_artifacts

    exit 1
  }

  collect_generated_artifacts

  cp -Rf result/* "${ARTIFACTS_DIR}/"
  # The Nix build creates glove80_lh and glove80_rh directories, not lf and rh
  # We rename glove80_lh to glove80_lf in default.nix
  cp ${ARTIFACTS_DIR}/glove80_lf/zmk.uf2 ${ARTIFACTS_DIR}/${BOARD_NAME}_lf.uf2
  cp ${ARTIFACTS_DIR}/glove80_rh/zmk.uf2 ${ARTIFACTS_DIR}/${BOARD_NAME}_rh.uf2
  log_info "Build artifacts saved to $ARTIFACTS_DIR/"
  log_info "Firmware available at ${ARTIFACTS_DIR}/${BOARD_NAME}.uf2"

  # Handle CI output variables
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    # GitHub Actions
    {
      echo "firmware_path=${ARTIFACTS_DIR}/${BOARD_NAME}.uf2"
      echo "artifacts_dir=$ARTIFACTS_DIR"
      echo "build_id=$BUILD_ID"
      echo "repository=$REPO"
      echo "branch=$BRANCH"
      echo "commit=$GIT_COMMIT"
    } >>"$GITHUB_OUTPUT"
  elif [ -n "${GLOVEBOX_BUILD:-}" ]; then
    # Glovebox
    {
      echo "firmware_path=${ARTIFACTS_DIR}/${BOARD_NAME}.uf2"
      echo "artifacts_dir=$ARTIFACTS_DIR"
      echo "build_id=$BUILD_ID"
      echo "repository=$REPO"
      echo "branch=$BRANCH"
      echo "commit=$GIT_COMMIT"
    } >>"${ARTIFACTS_DIR}/build.env"
  fi
  log_info "Build process completed successfully"
}

# Run main
main "$@"
