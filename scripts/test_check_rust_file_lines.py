"""Tests for the Rust source physical-line limit checker."""

from __future__ import annotations

import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_rust_file_lines", Path(__file__).with_name("check_rust_file_lines.py")
)
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


class RustFileSizeTests(unittest.TestCase):
    def test_counts_empty_final_line_and_crlf(self):
        samples = [
            (b"", 0),
            (b"x", 1),
            (b"x\n", 1),
            (b"x\n\n", 2),
            (b"x\r\ny\r\n", 2),
            (b"// comment\n\nfn test() {}", 3),
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.rs"
            for data, expected in samples:
                with self.subTest(data=data):
                    path.write_bytes(data)
                    self.assertEqual(checker.physical_line_count(path), expected)

    def test_400_passes_and_401_fails_without_final_newline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "allowed.rs").write_bytes(b"// line\n" * 400)
            (root / "oversized.rs").write_bytes(b"// line\n" * 400 + b"// final")
            count, over_limit = checker.violations(root)
            self.assertEqual(count, 2)
            self.assertEqual(over_limit, [(Path("oversized.rs"), 401)])

    def test_includes_tests_examples_benches_and_build_scripts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ["src/lib.rs", "tests/lifecycle.rs", "examples/app.rs", "benches/mix.rs", "build.rs"]
            for name in names:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"\n" * 401)
            count, over_limit = checker.violations(root)
            self.assertEqual(count, len(names))
            self.assertEqual({path.as_posix() for path, _ in over_limit}, set(names))

    def test_excludes_markdown_build_output_vendor_and_reference_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = [
                "docs/ARCHITECTURE.md", "target/generated.rs", "crates/solouder-core/target/generated.rs",
                ".git/objects/cache.rs", "vendor/external.rs", "reference/soloud/glue/external.rs",
                "crates/solouder-core/src/features/vendor.rs",
            ]
            for name in names:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"\n" * 401)
            count, over_limit = checker.violations(root)
            self.assertEqual(count, 1)
            self.assertEqual(over_limit[0][0].as_posix(), names[-1])

    def test_invalid_root_returns_error(self):
        with tempfile.TemporaryDirectory() as directory:
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(checker.main(["--root", str(Path(directory) / "missing")]), 2)

    def test_failure_and_empty_tree_exit_codes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(checker.main(["--root", str(root)]), 0)
                (root / "oversized.rs").write_bytes(b"\n" * 401)
                self.assertEqual(checker.main(["--root", str(root)]), 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
