# Native package-manager metadata

`tidas-dist metadata` generates Homebrew and Winget manifests from the
SHA-256 sidecars of the same five archives published by the release workflow.
The package-manager paths never rebuild `tidas`.

For a completed `v0.1.0` artifact set:

```bash
cargo run --locked -p tidas-dist -- metadata \
  --release-base-url \
  https://github.com/tiangong-lca/tidas-tools/releases/download/v0.1.0 \
  --artifacts-dir dist/artifacts \
  --output-dir dist/package-metadata
```

The output is:

- `homebrew/tidas.rb`, ready to copy into an approved `homebrew-*` tap;
- the three `winget/TianGong.Tidas*.yaml` manifests, ready for `winget
  validate` and a separately approved community submission.

The release workflow validates and uploads these generated files with every
tag. Creating an external tap repository or submitting to `microsoft/winget-pkgs`
is intentionally a separate, human-approved publication action.
