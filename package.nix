{
  lib,
  stdenvNoCC,
  fetchurl,
}:

let
  data = builtins.fromJSON (builtins.readFile ./version.json);
  system = stdenvNoCC.hostPlatform.system;

  asset =
    {
      x86_64-linux = "ai-commit-linux-x86_64";
    }
    .${system} or (throw "ai-commit: unsupported system ${system}");
in
stdenvNoCC.mkDerivation {
  pname = "ai-commit";
  version = data.version;

  # Static musl binary with zero /nix/store references, so a plain fixed-output
  # fetch is enough.
  src = fetchurl {
    url = "https://github.com/atomic-235/ai-commit-flake/releases/download/v${data.version}/${asset}";
    hash = data.assets.${system};
  };

  dontUnpack = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 "$src" "$out/bin/ai-commit"
    runHook postInstall
  '';

  meta = with lib; {
    description = "Generate a Conventional Commits message for staged changes (static release binary)";
    homepage = "https://github.com/atomic-235/ai-commit-flake";
    license = licenses.mit;
    platforms = [ "x86_64-linux" ];
    mainProgram = "ai-commit";
  };
}
