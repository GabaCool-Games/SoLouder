# SoLouder contributors

People who contributed to SoLouder: the Rust port and the patched C++
reference it is tested against. Authors of the original SoLoud are credited in
[UPSTREAM_AUTHORS.md](UPSTREAM_AUTHORS.md).

Add yourself in your first pull request (see [CONTRIBUTING.md](CONTRIBUTING.md)).

- Simon Bauer
- kelo221 — https://github.com/kelo221

## Upstream fixes adopted in the C++ reference

These fixes come from unmerged upstream SoLoud pull requests. Their authors
did not contribute to SoLouder directly. Each patch in `reference/patches/`
names its source.

- Jake S. Del Mastro (pgrAm) — WavStream channel stride and looping `hasEnded`, upstream PR #379
- Yotam Gingold (yig) — non-copyable `Wav`, upstream PR #375
- valid-ptr — diagnosis of the pre-init voice-array crash, upstream PR #345
