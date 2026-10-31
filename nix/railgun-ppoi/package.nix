# RAILGUN's proof-of-innocence node. MongoDB and network configuration are
# supplied by the caller when the service runs.
{ pkgs }:

let
  src = pkgs.fetchFromGitHub {
    owner = "Railgun-Community";
    repo = "private-proof-of-innocence";
    rev = "4b1eaf6ef19099dbfd6b43b1ca78d2ce0132a752";
    hash = "sha256-hlPF9dqu0isfdqiEoAU0+unI2sInZl8q2XQXwdyN2oI=";
  };
in
pkgs.stdenv.mkDerivation {
  pname = "railgun-ppoi";
  version = "4b1eaf6";
  inherit src;
  sourceRoot = "source/packages/node";

  # fetchYarnDeps caches the pinned upstream lockfile, including its GitHub
  # ethers fork. The cache hash is independent of the build platform.
  yarnOfflineCache = pkgs.fetchYarnDeps {
    yarnLock = "${src}/packages/node/yarn.lock";
    hash = "sha256-Y0Szi9LXfsfTCFUTVSQephjGXe3ocLhBz28nTJIXFQI=";
  };

  nativeBuildInputs = [
    pkgs.nodejs_22
    pkgs.yarnConfigHook
    pkgs.makeWrapper
    pkgs.python3
  ];
  # The output contains thousands of JS files; generic fixup scans each one.
  dontFixup = true;

  buildPhase = ''
    runHook preBuild
    # yarnConfigHook installs offline without scripts. Build the native LevelDB
    # binding here for this platform before compiling the TypeScript service.
    npm rebuild leveldown --offline
    yarn build
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/lib/railgun-ppoi" "$out/bin"
    mv dist node_modules "$out/lib/railgun-ppoi/"
    cp package.json "$out/lib/railgun-ppoi/"
    makeWrapper ${pkgs.nodejs_22}/bin/node "$out/bin/railgun-ppoi" \
      --add-flags "$out/lib/railgun-ppoi/dist/main.js"
    runHook postInstall
  '';

  meta = {
    description = "RAILGUN proof-of-innocence node";
    homepage = "https://github.com/Railgun-Community/private-proof-of-innocence";
    license = pkgs.lib.licenses.mit;
    mainProgram = "railgun-ppoi";
  };
}
