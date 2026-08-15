#!/usr/bin/env python3
"""Update every dependency surface in the TonePush repository.

This is intentionally a local, on-demand maintenance command.  It updates
manifest constraints as well as lockfiles, then runs the checks that expose the
source migrations an incompatible update may require.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
USER_AGENT = "tonepush-dependency-updater/1"


def command_text(command: list[str]) -> str:
    return " ".join(command)


def run(
    command: list[str],
    *,
    cwd: Path = ROOT,
    capture: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    if not capture:
        print(f"\n$ {command_text(command)}", flush=True)
    result = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        check=False,
    )
    if check and result.returncode != 0:
        if capture:
            if result.stdout:
                print(result.stdout, end="", file=sys.stderr)
            if result.stderr:
                print(result.stderr, end="", file=sys.stderr)
        raise subprocess.CalledProcessError(result.returncode, command)
    return result


def require_commands(commands: list[str]) -> None:
    missing = [command for command in commands if shutil.which(command) is None]
    if missing:
        names = ", ".join(missing)
        raise SystemExit(f"missing required command(s): {names}")


def tracked_files() -> list[Path]:
    output = run(["git", "ls-files"], capture=True).stdout
    return [ROOT / name for name in output.splitlines()]


def require_clean_tree(allow_dirty: bool) -> None:
    if allow_dirty:
        return
    status = run(
        ["git", "status", "--porcelain", "--untracked-files=normal"], capture=True
    ).stdout
    if status:
        raise SystemExit(
            "the working tree is not clean; commit or stash it first "
            "(or pass --allow-dirty if you accept mixed changes)"
        )


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read().decode("utf-8")


def numeric_version(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in value.split("."))


_SIMPLE_REQUIREMENT = re.compile(
    r"^(?P<prefix>\^|~|=)?(?P<space>\s*)(?P<version>\d+(?:\.\d+){0,2})$"
)
_RUBY_REQUIREMENT = re.compile(
    r"^(?P<prefix>~>|>=|<=|=|>|<)?(?P<space>\s*)(?P<version>\d+(?:\.\d+){0,2})$"
)


def raised_requirement(current: str, latest: str, *, ruby: bool = False) -> str:
    """Move a simple requirement to the latest release, preserving its style."""
    pattern = _RUBY_REQUIREMENT if ruby else _SIMPLE_REQUIREMENT
    match = pattern.fullmatch(current)
    if match is None:
        print(f"  warning: cannot safely rewrite complex constraint {current!r}")
        return current

    old_version = match.group("version")
    precision = len(old_version.split("."))
    latest_parts = latest.split(".")
    if match.group("prefix") == "=":
        precision = len(latest_parts)
    replacement = ".".join(latest_parts[:precision])
    return f"{match.group('prefix') or ''}{match.group('space')}{replacement}"


def latest_rust() -> str:
    manifest = fetch_text("https://static.rust-lang.org/dist/channel-rust-stable.toml")
    match = re.search(
        r'^\[pkg\.rust\]\s*$.*?^version = "(?P<version>\d+\.\d+\.\d+)',
        manifest,
        flags=re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise RuntimeError("could not read the current stable Rust version")
    return match.group("version")


def update_rust_toolchain() -> int:
    path = ROOT / "rust-toolchain.toml"
    current_text = path.read_text()
    latest = latest_rust()
    new_text, count = re.subn(
        r'(?m)^(channel\s*=\s*)"\d+\.\d+\.\d+"',
        rf'\1"{latest}"',
        current_text,
        count=1,
    )
    if count != 1:
        raise RuntimeError("could not find the pinned Rust channel")
    if new_text == current_text:
        print(f"Rust toolchain: {latest} (current)")
        return 0
    path.write_text(new_text)
    print(f"Rust toolchain: updated to {latest}")
    return 1


def workspace_package_names() -> set[str]:
    metadata = run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], capture=True
    )
    return {package["name"] for package in json.loads(metadata.stdout)["packages"]}


def cargo_manifest_dependencies(
    path: Path, internal_packages: set[str]
) -> list[tuple[str, str, int, int]]:
    """Return (crate, constraint, start, end) spans in one Cargo manifest."""
    text = path.read_text()
    dependencies: list[tuple[str, str, int, int]] = []
    section = ""
    offset = 0

    for line in text.splitlines(keepends=True):
        heading = re.match(r"^\s*\[([^]]+)]\s*(?:#.*)?$", line)
        if heading:
            section = heading.group(1)

        in_dependencies = section == "dependencies" or section.endswith(".dependencies")
        declaration = re.match(
            r'^\s*(?P<name>[A-Za-z0-9_.-]+)\s*=\s*(?P<value>.*)$', line
        )
        if in_dependencies and declaration:
            name = declaration.group("name")
            value = declaration.group("value")
            package_match = re.search(r'\bpackage\s*=\s*"([^"]+)"', value)
            package = package_match.group(1) if package_match else name
            if package not in internal_packages and not re.search(r"\bpath\s*=", value):
                simple = re.match(r'"(?P<constraint>[^"]+)"', value)
                inline = re.search(r'\bversion\s*=\s*"(?P<constraint>[^"]+)"', value)
                requirement = simple or inline
                if requirement:
                    start = offset + declaration.start("value") + requirement.start(
                        "constraint"
                    )
                    end = offset + declaration.start("value") + requirement.end(
                        "constraint"
                    )
                    dependencies.append((package, requirement.group("constraint"), start, end))
        offset += len(line)
    return dependencies


def latest_crate(crate: str) -> str:
    result = run(
        ["cargo", "search", crate, "--limit", "1", "--registry", "crates-io"],
        capture=True,
    )
    match = re.search(rf'(?m)^{re.escape(crate)}\s*=\s*"([^"]+)"', result.stdout)
    if match is None:
        raise RuntimeError(f"could not find the latest crates.io release of {crate}")
    return match.group(1)


def update_cargo_manifests(files: list[Path]) -> int:
    internal_packages = workspace_package_names()
    manifests = [path for path in files if path.name == "Cargo.toml"]
    by_manifest: dict[Path, list[tuple[str, str, int, int]]] = {
        path: cargo_manifest_dependencies(path, internal_packages) for path in manifests
    }
    crates = sorted(
        {crate for dependencies in by_manifest.values() for crate, _, _, _ in dependencies}
    )
    latest = {crate: latest_crate(crate) for crate in crates}
    changes = 0

    for path, dependencies in by_manifest.items():
        text = path.read_text()
        replacements: list[tuple[int, int, str, str, str]] = []
        for crate, current, start, end in dependencies:
            replacement = raised_requirement(current, latest[crate])
            if replacement != current:
                replacements.append((start, end, replacement, crate, current))
        for start, end, replacement, crate, current in reversed(replacements):
            text = text[:start] + replacement + text[end:]
            print(
                f"{path.relative_to(ROOT)}: {crate} {current} -> {replacement} "
                f"(latest {latest[crate]})"
            )
        if replacements:
            path.write_text(text)
            changes += len(replacements)

    if changes == 0:
        print(f"Cargo manifests: {len(crates)} external crates are on current release lines")
    run(["cargo", "update"])
    return changes


_GEM_DECLARATION = re.compile(
    r'(?m)^\s*gem\s+["\'](?P<name>[A-Za-z0-9_.-]+)["\']'
    r'(?:\s*,\s*["\'](?P<constraint>[^"\']+)["\'])?'
)
_GEMSPEC_DECLARATION = re.compile(
    r'(?m)^\s*\w+\.add_(?:runtime_)?dependency\s+["\']'
    r'(?P<name>[A-Za-z0-9_.-]+)["\']'
    r'(?:\s*,\s*["\'](?P<constraint>[^"\']+)["\'])?'
)


def latest_gem(name: str) -> str:
    result = run(["gem", "search", "--remote", "--exact", name], capture=True)
    match = re.search(rf'(?m)^{re.escape(name)} \(([^, )]+)', result.stdout)
    if match is None:
        raise RuntimeError(f"could not find the latest RubyGems release of {name}")
    return match.group(1)


def update_ruby_manifests(files: list[Path]) -> int:
    manifests = [
        path for path in files if path.name == "Gemfile" or path.suffix == ".gemspec"
    ]
    declarations: dict[Path, list[re.Match[str]]] = {}
    gem_names: set[str] = set()
    for path in manifests:
        text = path.read_text()
        pattern = _GEM_DECLARATION if path.name == "Gemfile" else _GEMSPEC_DECLARATION
        matches = list(pattern.finditer(text))
        declarations[path] = matches
        gem_names.update(match.group("name") for match in matches)

    latest = {name: latest_gem(name) for name in sorted(gem_names)}
    changes = 0
    for path, matches in declarations.items():
        text = path.read_text()
        replacements: list[tuple[int, int, str, str, str]] = []
        for match in matches:
            current = match.group("constraint")
            if current is None:
                continue
            name = match.group("name")
            replacement = raised_requirement(current, latest[name], ruby=True)
            if replacement != current:
                start, end = match.span("constraint")
                replacements.append((start, end, replacement, name, current))
        for start, end, replacement, name, current in reversed(replacements):
            text = text[:start] + replacement + text[end:]
            print(
                f"{path.relative_to(ROOT)}: {name} {current} -> {replacement} "
                f"(latest {latest[name]})"
            )
        if replacements:
            path.write_text(text)
            changes += len(replacements)

    if changes == 0:
        print(f"Ruby manifests: {len(gem_names)} direct gems are on current release lines")
    for gemfile in sorted(path for path in manifests if path.name == "Gemfile"):
        run(["bundle", "update", "--all"], cwd=gemfile.parent)
    return changes


def latest_ruby() -> str:
    index = fetch_text("https://cache.ruby-lang.org/pub/ruby/index.txt")
    versions = {
        match.group(1)
        for match in re.finditer(r"(?m)^ruby-(\d+\.\d+\.\d+)\s", index)
    }
    if not versions:
        raise RuntimeError("could not read the current stable Ruby version")
    return max(versions, key=numeric_version)


def update_ruby_toolchain(workflows: list[Path]) -> int:
    latest = latest_ruby()
    release_line = ".".join(latest.split(".")[:2])
    changes = 0
    pattern = re.compile(r'(ruby-version:\s*["\'])(\d+\.\d+)(["\'])')
    for path in workflows:
        text = path.read_text()
        new_text, count = pattern.subn(rf"\g<1>{release_line}\g<3>", text)
        if new_text != text:
            path.write_text(new_text)
            changes += count
            print(f"{path.relative_to(ROOT)}: Ruby -> {release_line} (latest {latest})")
    if changes == 0:
        print(f"Ruby toolchain: {release_line} (latest {latest})")
    return changes


def latest_action_major(repository: str) -> int:
    result = run(
        [
            "git",
            "ls-remote",
            "--tags",
            "--refs",
            f"https://github.com/{repository}.git",
            "v[0-9]*",
        ],
        capture=True,
    )
    versions: list[tuple[int, ...]] = []
    for line in result.stdout.splitlines():
        tag = line.rsplit("refs/tags/", 1)[-1]
        if re.fullmatch(r"v\d+(?:\.\d+)*", tag):
            versions.append(numeric_version(tag[1:]))
    if not versions:
        raise RuntimeError(f"could not find stable version tags for {repository}")
    return max(versions)[0]


_ACTION = re.compile(
    r"(?P<prefix>\buses:\s*)(?P<repository>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)"
    r"(?P<subpath>/[A-Za-z0-9_./-]+)?@v(?P<major>\d+)"
)


def update_actions(workflows: list[Path]) -> int:
    repositories: set[str] = set()
    for path in workflows:
        repositories.update(
            match.group("repository") for match in _ACTION.finditer(path.read_text())
        )
    latest = {
        repository: latest_action_major(repository) for repository in sorted(repositories)
    }
    changes = 0
    for path in workflows:
        text = path.read_text()

        def replacement(match: re.Match[str]) -> str:
            nonlocal changes
            repository = match.group("repository")
            current = int(match.group("major"))
            wanted = latest[repository]
            if current == wanted:
                return match.group(0)
            changes += 1
            print(f"{path.relative_to(ROOT)}: {repository} v{current} -> v{wanted}")
            subpath = match.group("subpath") or ""
            return f"{match.group('prefix')}{repository}{subpath}@v{wanted}"

        new_text = _ACTION.sub(replacement, text)
        if new_text != text:
            path.write_text(new_text)
    if changes == 0:
        print(f"GitHub Actions: {len(repositories)} actions are on current major versions")
    return changes


def verify() -> None:
    run(["cargo", "fmt", "--all", "--check"])
    run(["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
    run(["cargo", "test", "--workspace"])

    ruby = ROOT / "crates" / "hx-ruby"
    run(["bundle", "exec", "rake", "compile"], cwd=ruby)
    run(["gem", "build", "hx_ruby.gemspec"], cwd=ruby)
    run(
        [
            "bundle",
            "exec",
            "ruby",
            "-Ilib",
            "-e",
            'require "hx_ruby"; puts HxRuby::VERSION',
        ],
        cwd=ruby,
    )

    with tempfile.TemporaryDirectory(prefix="tonepush-docs-") as destination:
        run(
            ["bundle", "exec", "jekyll", "build", "--destination", destination],
            cwd=ROOT / "docs",
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="update manifests and locks without building or testing",
    )
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="allow dependency changes to be mixed into an existing working tree",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    require_commands(["bundle", "cargo", "gem", "git"])
    require_clean_tree(args.allow_dirty)
    (ROOT / "target").mkdir(exist_ok=True)

    # Cargo, rustdoc, compilers, and Bundler can all write large intermediates
    # to /tmp. Keep this run self-contained under the already-ignored target/
    # tree and remove the temporary workspace on success or failure.
    with tempfile.TemporaryDirectory(
        prefix="dependency-update-", dir=ROOT / "target"
    ) as temporary_workspace:
        os.environ["TMPDIR"] = temporary_workspace
        files = tracked_files()
        workflows = [
            path
            for path in files
            if path.parent == ROOT / ".github" / "workflows"
            and path.suffix in {".yml", ".yaml"}
        ]

        print("Checking toolchains, manifests, lockfiles, and CI actions...\n")
        changes = 0
        changes += update_rust_toolchain()
        changes += update_cargo_manifests(files)
        changes += update_ruby_toolchain(workflows)
        changes += update_ruby_manifests(files)
        changes += update_actions(workflows)

        if args.no_verify:
            print("\nDependency update complete; verification was skipped.")
        else:
            print("\nDependencies updated. Running the full verification suite...")
            verify()
            print("\nDependency update and verification complete.")
        print(f"Direct manifest/toolchain changes made: {changes}")
        print("Review with: git diff --stat && git diff")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print(f"\nDependency update failed: {error}", file=sys.stderr)
        print(
            "Any changes made before the failure were left in the working tree for review.",
            file=sys.stderr,
        )
        raise SystemExit(1)
