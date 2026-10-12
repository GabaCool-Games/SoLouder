# Port status

What the Rust port does today, where it deliberately differs from the C++
reference, and how it is verified. Per-behavior status lives in
[parity-ledger.csv](parity-ledger.csv); the plan is in
[PORTING.md](PORTING.md#milestones-and-acceptance-gates).

## Summary

Milestone A (foundation and reference harness). The mixer validates its
configuration and renders silence. No audio sources, device output or Bevy
integration exist yet.

| Crate | Slices | Contents |
|---|---|---|
| `solouder-core` | `playback` | `MixerConfig` (validation), `Mixer::mix_interleaved` (silence) |
| `solouder-parity` | — | Scenario parser, Rust runner, comparison with C++ output |

Ledger: 2 behaviors `in-progress` (`Soloud::init`, `Soloud::mix`), the rest
`todo`.

## Verified by scenario

| Scenario | Checks | Result against C++ |
|---|---|---|
| `init_validation` | Accepted and rejected channel counts and buffer sizes; 0 selects the default rate and buffer; a failed init leaves nothing to mix | Identical log and samples |
| `silence_block_sizes` | Mixing with no voices for 1, 2, 4, 6 and 8 channels and blocks of 0 to 4096 frames | 47,097 samples, max difference 0 |

Run with `python scripts/run_parity.py`.

## Deliberate differences from the reference

| Area | C++ reference | Rust | Why |
|---|---|---|---|
| Mix length | `Soloud::mix` writes past its scratch buffer when asked for more frames than it holds | `Mixer::mix_interleaved` accepts any length | Memory safety; see the open-defect table in PORTING.md |
| Partial frames | Not representable (the C API takes a frame count) | A trailing partial frame in the output slice is set to silence | Rust takes an interleaved slice |
| Sample rate 0 | Means "use the default" (44100) in `Soloud::init` | `MixerConfig` rejects 0; the C API's "automatic" meaning belongs to the caller (the parity runner maps 0 to 44100) | No hidden defaults in the core type |

## Not yet covered

Everything else in the ledger, including init flags (`CLIP_ROUNDOFF`,
`LEFT_HANDED_3D`, ...), device backends, and `mixSigned16`.
