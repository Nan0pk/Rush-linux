from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}")
    target.write_text(text.replace(old, new))


replace_once(
    "tools/build-mkosi-image.sh",
    '''for build_tool in cargo mkosi; do
    if ! command -v "${build_tool}" >/dev/null 2>&1; then
        echo "Missing build tool: ${build_tool}. See docs/build-system.md; --plan needs neither tool." >&2
        exit 2
    fi
done
cd "${REPO_ROOT}"
''',
    '''for build_tool in cargo mkosi; do
    if ! command -v "${build_tool}" >/dev/null 2>&1; then
        echo "Missing build tool: ${build_tool}. See docs/build-system.md; --plan needs neither tool." >&2
        exit 2
    fi
done

echo "Build tool versions:"
printf '  cargo: %s\\n' "$(cargo --version)"
printf '  mkosi: %s\\n' "$(mkosi --version)"
echo "Resolved mkosi configuration:"
(
    cd "${MKOSI_DIR}"
    mkosi summary "${MKOSI_ARGS[@]}"
)
echo ""

cd "${REPO_ROOT}"
''',
)

replace_once(
    "tools/test-build-mkosi-base-boundary.py",
    '''    cargo = bin_dir / "cargo"
    cargo.write_text("#!/bin/sh\\nmkdir -p target/release\\nprintf fixture > target/release/optid\\nprintf fixture > target/release/optctl\\n")
    mkosi = bin_dir / "mkosi"
    mkosi.write_text(
        "#!/usr/bin/env python3\\nimport json, os, pathlib, sys\\n"
        "pathlib.Path(os.environ['RUSH_TEST_TRACE']).write_text(json.dumps({'args': sys.argv[1:], 'cwd': os.getcwd()}))\\n"
    )
''',
    '''    cargo = bin_dir / "cargo"
    cargo.write_text(
        "#!/bin/sh\\n"
        "if [ \"${1:-}\" = \"--version\" ]; then echo 'cargo 99-test'; exit 0; fi\\n"
        "mkdir -p target/release\\n"
        "printf fixture > target/release/optid\\n"
        "printf fixture > target/release/optctl\\n"
    )
    mkosi = bin_dir / "mkosi"
    mkosi.write_text(
        "#!/usr/bin/env python3\\nimport json, os, pathlib, sys\\n"
        "trace = pathlib.Path(os.environ['RUSH_TEST_TRACE'])\\n"
        "with trace.open('a') as handle: handle.write(json.dumps({'args': sys.argv[1:], 'cwd': os.getcwd()}) + '\\\\n')\\n"
        "if sys.argv[1:] == ['--version']: print('mkosi 99-test')\\n"
    )
''',
)

replace_once(
    "tools/test-build-mkosi-base-boundary.py",
    '''    recorded = json.loads(trace.read_text())
    assert recorded == {
        "args": ["build", "--force", "--snapshot=20260904",
                 f"--package-directory={local_packages}",
                 f"--package-directory={second_packages}",
                 f"--cache-dir={tmp_path / 'cache with spaces'}"],
        "cwd": str(repo / "mkosi"),
    }
''',
    '''    recorded = [json.loads(line) for line in trace.read_text().splitlines()]
    expected_inputs = [
        "--force",
        "--snapshot=20260904",
        f"--package-directory={local_packages}",
        f"--package-directory={second_packages}",
        f"--cache-dir={tmp_path / 'cache with spaces'}",
    ]
    assert recorded == [
        {"args": ["--version"], "cwd": str(repo)},
        {"args": ["summary", *expected_inputs], "cwd": str(repo / "mkosi")},
        {"args": ["build", *expected_inputs], "cwd": str(repo / "mkosi")},
    ]
    assert "cargo: cargo 99-test" in result.stdout
    assert "mkosi: mkosi 99-test" in result.stdout
    assert "Resolved mkosi configuration:" in result.stdout
''',
)
