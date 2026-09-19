# Packages tornadocash's `tornado-relayer` (https://github.com/tornadocash/tornado-relayer),
# patched to target a pool on a local test chain via env vars. See config.patch/worker.patch.
{ pkgs }:

let
  # Network-enabled fixed-output derivation, since buildNpmPackage's offline cache can't
  # resolve this dependency tree's git commits. Separate from the package below because FODs
  # can't reference other store paths.
  installed = pkgs.stdenvNoCC.mkDerivation {
    pname = "tornado-relayer-npm-install";
    version = "4.1.4";

    src = pkgs.fetchFromGitHub {
      owner = "tornadocash";
      repo = "tornado-relayer";
      rev = "v4.1.4";
      hash = "sha256-yaLHtNXmTDdTEQTTtxHv7Ol48kP8VLSbOfc0e7E8Brk=";
    };

    patches = [
      ./config.patch
      ./worker.patch
    ];

    nativeBuildInputs = [
      pkgs.nodejs
      pkgs.cacert
      pkgs.git
    ];

    dontConfigure = true;
    # patchShebangs would inject /nix/store references, which FODs can't have.
    dontFixup = true;

    buildPhase = ''
      runHook preBuild
      export HOME="$TMPDIR"
      export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      npm install --no-audit --no-fund --ignore-scripts --omit=dev
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      mkdir -p "$out"
      cp -r . "$out"
      runHook postInstall
    '';

    outputHashMode = "recursive";
    outputHashAlgo = "sha256";
    outputHash = "sha256-D/2Jcze0MK1xm5+jW6Z53elUv+GLcRWDk6NMoU+lV4w=";
  };
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "tornado-relayer";
  version = "4.1.4";

  dontUnpack = true;
  nativeBuildInputs = [ pkgs.makeWrapper ];

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"
    for name in server worker treeWatcher priceWatcher healthWatcher; do
      makeWrapper ${pkgs.nodejs}/bin/node "$out/bin/tornado-relayer-$name" \
        --add-flags "${installed}/src/$name.js"
    done
    runHook postInstall
  '';

  meta = {
    description = "Relayer for the Tornado Cash privacy solution, patched to target a pool deployed on a local test chain";
    homepage = "https://github.com/tornadocash/tornado-relayer";
    license = pkgs.lib.licenses.mit;
  };
}
