"""Hermetic checks for the Veritas build-context staging contract."""

from __future__ import annotations

import os
import pathlib
import shlex
import shutil
import stat
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).with_name("stage-veritas.sh")
DOCKERFILE = SCRIPT.with_name("Dockerfile")
DEPLOY_SCRIPT = SCRIPT.with_name("deploy.sh")
SOURCE_COMMIT = "e40e11f96712806644a586a9ee8895548efdebd9"
NAMES = ("executor-binary", "validator-binary")


def write_elf_fixture(path: pathlib.Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    header = bytearray(64)
    header[:4] = b"\x7fELF"
    header[4:6] = b"\x02\x01"  # 64-bit, little-endian
    header[18:20] = b"\x3e\x00"  # x86-64
    path.write_bytes(header + bytes(65536 - len(header)))


class StageVeritasTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.source = self.root / "source"
        self.destination = self.root / "staged"

    def run_stage(self, *args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), "--destination", str(self.destination), *args],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )

    def test_explicit_custom_override_stages_nested_executables(self) -> None:
        for name in NAMES:
            write_elf_fixture(self.source / name / name)

        result = self.run_stage("--source-dir", str(self.source), "--custom")
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in NAMES:
            staged = self.destination / name / name
            self.assertEqual(staged.read_bytes(), (self.source / name / name).read_bytes())
            self.assertTrue(staged.stat().st_mode & stat.S_IXUSR)

    def test_custom_override_rejects_missing_binary_without_placeholder(self) -> None:
        write_elf_fixture(self.source / NAMES[0] / NAMES[0])

        result = self.run_stage("--source-dir", str(self.source), "--custom")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing Veritas binary", result.stderr)
        for name in NAMES:
            self.assertFalse((self.destination / name / name).exists())

    def test_pinned_source_rejects_modified_binary(self) -> None:
        for name in NAMES:
            write_elf_fixture(self.source / name / name)

        result = self.run_stage("--source-dir", str(self.source))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertFalse((self.destination / NAMES[0] / NAMES[0]).exists())

    def test_mock_download_uses_commit_pinned_url_and_rejects_tamper(self) -> None:
        payload = self.root / "payload"
        write_elf_fixture(payload)
        mock_bin = self.root / "mock-bin"
        mock_bin.mkdir()
        log = self.root / "urls"
        mock_curl = mock_bin / "curl"
        mock_curl.write_text(
            "#!/usr/bin/env python3\n"
            "import os, pathlib, shutil, sys\n"
            "pathlib.Path(os.environ['MOCK_CURL_LOG']).write_text(sys.argv[-1] + '\\n')\n"
            "shutil.copyfile(os.environ['MOCK_CURL_PAYLOAD'], "
            "sys.argv[sys.argv.index('--output') + 1])\n"
        )
        mock_curl.chmod(0o755)
        env = os.environ.copy()
        env.update(
            PATH=f"{mock_bin}:{env['PATH']}",
            MOCK_CURL_LOG=str(log),
            MOCK_CURL_PAYLOAD=str(payload),
        )

        result = self.run_stage(env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertEqual(
            log.read_text().strip(),
            f"https://raw.githubusercontent.com/one-covenant/basilica/{SOURCE_COMMIT}/"
            "bin/executor-binary/executor-binary",
        )
        self.assertFalse((self.destination / NAMES[0] / NAMES[0]).exists())

    def test_build_script_stages_custom_override_into_docker_context(self) -> None:
        for name in NAMES:
            write_elf_fixture(self.source / name / name)
        workspace = self.root / "workspace"
        scripts = workspace / "scripts" / "validator"
        scripts.mkdir(parents=True)
        shutil.copy2(SCRIPT, scripts / SCRIPT.name)
        shutil.copy2(SCRIPT.with_name("build.sh"), scripts / "build.sh")
        mock_bin = self.root / "mock-bin"
        mock_bin.mkdir()
        docker_log = self.root / "docker-args"
        mock_docker = mock_bin / "docker"
        mock_docker.write_text(
            "#!/bin/sh\n"
            "printf '%s\\n' \"$@\" > \"$MOCK_DOCKER_LOG\"\n"
        )
        mock_docker.chmod(0o755)
        env = os.environ.copy()
        env.update(PATH=f"{mock_bin}:{env['PATH']}", MOCK_DOCKER_LOG=str(docker_log))

        result = subprocess.run(
            [
                "bash",
                str(scripts / "build.sh"),
                "--no-extract",
                "--veritas-binaries",
                str(self.source),
            ],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in NAMES:
            self.assertTrue((workspace / "bin" / name / name).is_file())
            self.assertFalse((workspace / name).exists())
        self.assertIn("VERITAS_BINARY_SOURCE=custom", docker_log.read_text())

    def test_dockerfile_pinned_branch_ignores_stale_custom_context(self) -> None:
        dockerfile = DOCKERFILE.read_text()
        self.assertIn("--source-dir /tmp/veritas-input --custom", dockerfile)
        pinned_branch = dockerfile.split(
            'elif [ "$VERITAS_BINARY_SOURCE" = pinned ]; then \\\n', 1
        )[1].split('    else \\\n', 1)[0]
        self.assertIn(
            "/usr/local/bin/stage-veritas --destination /opt/basilica/bin;",
            pinned_branch,
        )
        self.assertNotIn("--source-dir", pinned_branch)

    def test_ssh_deploy_uses_nested_paths_for_pinned_and_custom_files(self) -> None:
        deploy_source = DEPLOY_SCRIPT.read_text()
        deploy_function = "deploy_veritas_binaries() {" + deploy_source.split(
            "deploy_veritas_binaries() {", 1
        )[1].split("\ndeploy_binary() {", 1)[0]

        for directory, override in ((self.root / "bin", False), (self.root / "custom", True)):
            for name in NAMES:
                binary = directory / name / name
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"test binary")
            script = (
                "set -euo pipefail\n"
                f"VERITAS_BINARIES_DIR={shlex.quote(str(directory) if override else '')}\n"
                "log() { :; }\n"
                "ssh_cmd() { printf 'ssh:%s\\n' \"$1\"; }\n"
                "scp_file() { printf 'scp:%s=>%s\\n' \"$1\" \"$2\"; }\n"
                f"{deploy_function}\n"
                "deploy_veritas_binaries\n"
            )
            result = subprocess.run(
                ["bash", "-c", script],
                cwd=self.root,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            for name in NAMES:
                source_path = str(directory) if override else "./bin"
                self.assertIn(
                    f"scp:{source_path}/{name}/{name}=>/opt/basilica/bin/{name}/{name}",
                    result.stdout,
                )


if __name__ == "__main__":
    unittest.main()
