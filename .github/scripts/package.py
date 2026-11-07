"""Package a native release executable without losing Unix execute permissions."""

import argparse
import hashlib
from pathlib import Path
import tarfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target",
        required=True,
        choices=[
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ],
    )
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()

    windows = args.target.endswith("windows-msvc")
    executable = "tacit.exe" if windows else "tacit"
    binary = Path("target") / args.target / "release" / executable
    if not binary.is_file():
        parser.error(f"Release executable does not exist: {binary}")

    name = f"tacit-{args.target}"
    args.output_dir.mkdir(parents=True, exist_ok=True)
    archive = args.output_dir / f"{name}{'.zip' if windows else '.tar.gz'}"
    files = [(binary, f"{name}/{executable}"), (Path("README.md"), f"{name}/README.md")]
    if windows:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for source, destination in files:
                bundle.write(source, destination)
    else:
        with tarfile.open(archive, "w:gz") as bundle:
            for source, destination in files:
                bundle.add(source, arcname=destination)

    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    archive.with_name(f"{archive.name}.sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="utf-8"
    )
    print(archive)


if __name__ == "__main__":
    main()
