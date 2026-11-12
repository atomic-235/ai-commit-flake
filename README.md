# ai-commit-flake

`ai-commit` — generate a Conventional Commits message for staged changes via
the opencode CLI — built static by CI and published as a single release binary.

The source lives in this repo. CI builds it with `pkgsStatic`, so the release
binary is a static musl executable with zero `/nix/store` references.

## Packages

| Attribute | Contents |
|---|---|
| `packages.x86_64-linux.default` | the published static binary |
| `packages.x86_64-linux.ai-commit` | alias of `default` |
| `packages.x86_64-linux.static` | the `pkgsStatic` build CI publishes |

`overlays.default` provides `pkgs.ai-commit`.

## Usage

```nix
{
  inputs.ai-commit-flake.url = "github:atomic-235/ai-commit-flake";
  inputs.ai-commit-flake.inputs.nixpkgs.follows = "nixpkgs";
}
```

```nix
nixpkgs.overlays = [ inputs.ai-commit-flake.overlays.default ];
# pkgs.ai-commit
```

Runtime dependencies (`with-secrets`, `git`, `gum`) come from the consumer's
`PATH`, as they did when this was a Go program.

## Updates

CI runs on every push to `main` that touches `Cargo.toml` or `src/`: it reads
the version from `Cargo.toml`, skips when `v<version>` already exists (override
with `workflow_dispatch` and `force=true`), builds the static binary, publishes
it, records the asset hash in `version.json`, verifies `.#default` fetches it,
then commits and force-moves the `v<version>` and `latest` tags.

Release the next version by bumping `version` in `Cargo.toml` and pushing.

Supported: `x86_64-linux`.
