# based on https://github.com/moergo-sc/glove80-zmk-config
# docker buildx build -t glove80-zmk-config-nix:latest -f keyboards/glove80/toolchain/Dockerfile keyboards/glove80/toolchain/
FROM nixpkgs/nix:nixos-23.11

ENV PATH=/root/.nix-profile/bin:/usr/bin:/bin

SHELL ["/usr/bin/bash", "-c"]
#, "-o", "pipefail", "-c"]
RUN <<EOF
    set -euo pipefail
    nix-env -iA cachix -f https://cachix.org/api/v1/install
    cachix use moergo-glove80-zmk-dev
    mkdir /config
    # Mirror ZMK repository to make it easier to reference both branches and
    # tags without remote namespacing
    git clone --mirror https://github.com/moergo-sc/zmk /zmk
    GIT_DIR=/zmk git worktree add --detach /src
EOF

# Prepopulate the container's nix store with the build dependencies for the main
# branch and the most recent three tags
WORKDIR /src
RUN <<EOF
    set -euo pipefail
    for tag in main $(git tag -l --sort=committerdate | tail -n 3); do
      git checkout -q --detach $tag
      nix-shell --run true -A zmk ./default.nix
    done
EOF

RUN mkdir -p /workspace /app/bin
ENV PATH=/app/bin:$PATH
COPY --chmod=755 entrypoint.sh build.sh libutils.sh /app/bin/

ENV NIX_PATH=moergo-zmk=/src

ENTRYPOINT ["entrypoint.sh"]
CMD ["build.sh"]
