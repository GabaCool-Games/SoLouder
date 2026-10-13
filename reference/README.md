# C++ reference

SoLouder's behavior is checked against SoLoud's C++ engine. The reference is:

1. upstream SoLoud at the commit in [`upstream.json`](upstream.json), plus
2. the patches in [`patches/`](patches), applied in filename order.

`scripts/fetch_reference.py` builds it in `reference/soloud/`, which is
git-ignored. It writes a stamp file there, so rerunning the script is a no-op
until the pinned commit or the patches change; then use `--force`. Pass
`--source <url-or-path>` to clone from a mirror or a local copy instead of
GitHub.

## Changing the reference

1. Commit the change on a SoLoud branch that starts at the pinned commit
   (currently the working branch of the maintainer's SoLoud fork, published as
   `master` of [kelo221/soloud](https://github.com/kelo221/soloud)). Keep one
   fix per commit, and name the upstream issue or PR in the message. That
   branch also carries build scripts and a README after the code commits; pass
   `--ref <last code commit>` when exporting so they stay out of the patches.
2. Add a test to [`tests/regressions.cpp`](tests/regressions.cpp) and to the
   list in [`tests/CMakeLists.txt`](tests/CMakeLists.txt). It must fail on the
   code before your fix and pass after it.
3. Export and verify:

   ```sh
   python scripts/export_reference_patches.py <path-to-fork>
   python scripts/fetch_reference.py --force
   python scripts/test_reference.py
   ```

4. Add the patch to the patch ledger in
   [docs/PORTING.md](../docs/PORTING.md#reference-patch-ledger).

Patch files are stored byte-exact (`.gitattributes` marks them `-text`)
because some upstream files use CRLF line endings.

## Tests

[`tests/`](tests) is a standalone CMake project. It builds SoLoud as a static
library with only the null backend, so it needs no audio device and has no
audio thread, and runs each test in its own process. A crash or hang then fails
only that test.

```sh
python scripts/test_reference.py
# or by hand:
cmake -S reference/tests -B build/reference
cmake --build build/reference --config Release
ctest --test-dir build/reference -C Release --output-on-failure
```

To test another SoLoud tree, such as an unpatched checkout, pass
`--reference <path>` to `scripts/test_reference.py`.
