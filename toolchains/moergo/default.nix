{
  pkgs ? (import <moergo-zmk/nix/pinned-nixpkgs.nix> { }),
  moergo ? (import <moergo-zmk> { }),
  zmk ? moergo.zmk,
}:
let
  config = ./.;
  keymap = "${config}/glove80.keymap";
  kconfig = "${config}/glove80.conf";
  outputName = "glove80";

  customZmk = zmk.overrideAttrs (oldAttrs: {
    installPhase = ''
      ${oldAttrs.installPhase}
      cp  zephyr/include/generated/devicetree_generated.h "$out/";
    '';
  });

  combine =
    a: b: name:
    pkgs.runCommandNoCC "combined_firmware" { } ''
      mkdir -p $out
      echo "cat ${a}/zmk.uf2 ${b}/zmk.uf2 > $out/${name}.uf2"
      cat ${a}/zmk.uf2 ${b}/zmk.uf2 > $out/${name}.uf2
    '';

  collect_build_artifact =
    board_type: artifact:
    let
      result = pkgs.runCommandNoCC "collect_build_artifact_${board_type}" { } ''
        # Copy build files
        echo "Copying build files from ${artifact} to $out"
        cp -rT ${artifact} $out
      '';
    in
    result;

  glove80_left = customZmk.override {
    board = "glove80_lh";
    keymap = "${keymap}";
    kconfig = "${kconfig}";
  };
  left_processed = collect_build_artifact "lh" glove80_left;
  glove80_right = customZmk.override {
    board = "glove80_rh";
    keymap = "${keymap}";
    kconfig = "${kconfig}";
  };
  right_processed = collect_build_artifact "rh" glove80_right;

  # combined = moergo.combine_uf2 glove80_left glove80_right ${outputName};
  combined = combine left_processed right_processed "${outputName}";

in
pkgs.runCommand "${outputName}-firmware" { } ''
  set +x
  echo "finishing $out"
  mkdir -p $out

  # Copy firmware directories
  # The official nix build creates glove80_lh instead of glove80_lf 
  # We rename it here to follow west build
  cp -r ${left_processed} $out/glove80_lf
  cp -r ${right_processed} $out/glove80_rh

  cp ${combined}/${outputName}.uf2 $out/${outputName}.uf2
''
