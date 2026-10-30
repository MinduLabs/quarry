#!/usr/bin/env python3
"""The driver binaries of this checkout, for `just` recipes and CI.

`collect BUILD DRIVERS` copies every driver binary cargo built into BUILD (`target/debug` or
`target/release`) to DRIVERS (`target/drivers`), the one folder dev, tests and benchmarks read.

`refresh DRIVERS` replaces each driver already installed with the one in DRIVERS, so a dev run
never uses old driver code. It looks where the app keeps drivers (`driver_dir`). Drivers that
are not installed stay that way: the user downloads them from the app.

`serve DRIVERS PORT` answers the app's download like a release does: `/drivers.json` lists every
driver binary in DRIVERS with its SHA-256, and each binary is served by name. Only 127.0.0.1.
"""

import hashlib
import json
import os
import platform
import shutil
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

PREFIX = "quarry-driver-"


def binaries(directory: Path) -> list[Path]:
    """Driver binaries, not the `.d` files cargo writes next to them."""
    if not directory.is_dir():
        return []
    return sorted(
        f for f in directory.iterdir() if f.name.startswith(PREFIX) and "." not in f.name and f.is_file()
    )


def target() -> str:
    """The release target name the engine looks up for this machine."""
    machine = platform.machine().lower()
    if sys.platform == "darwin":
        return "universal-apple-darwin"
    if machine in ("x86_64", "amd64"):
        return "x86_64-unknown-linux-gnu"
    if machine in ("aarch64", "arm64"):
        return "aarch64-unknown-linux-gnu"
    raise SystemExit(f"drivers: no driver target for {sys.platform} {machine}")


def driver_dir() -> Path:
    """Where the engine keeps drivers: `QUARRY_DRIVER_DIR`, or `drivers` in the data directory
    (the `directories` crate's data dir plus `quarry`)."""
    if os.environ.get("QUARRY_DRIVER_DIR"):
        return Path(os.environ["QUARRY_DRIVER_DIR"])
    home = Path.home()
    if sys.platform == "darwin":
        data = home / "Library" / "Application Support"
    else:
        xdg = os.environ.get("XDG_DATA_HOME", "")
        data = Path(xdg) if os.path.isabs(xdg) else home / ".local" / "share"
    return data / "quarry" / "drivers"


def install(new: Path, old: Path) -> None:
    """Copied beside it, then renamed: a half-written driver is never loaded."""
    staging = old.with_name(f".{old.name}.part")
    shutil.copy2(new, staging)
    os.chmod(staging, 0o755)
    os.replace(staging, old)


def collect(build: Path, drivers: Path) -> None:
    built = binaries(build)
    if not built:
        raise SystemExit(f"drivers: no driver binaries in {build}")
    drivers.mkdir(parents=True, exist_ok=True)
    for new in built:
        install(new, drivers / new.name)


def refresh(built: Path) -> None:
    for old in binaries(driver_dir()):
        new = built / old.name
        if not new.is_file():
            continue
        install(new, old)
        print(f"drivers: refreshed {old.name}")


def serve(built: Path, port: int) -> None:
    files = {f"/{f.name}": f for f in binaries(built)}
    if not files:
        raise SystemExit(f"drivers: no driver binaries in {built}")
    base = f"http://127.0.0.1:{port}"
    manifest = json.dumps(
        {
            "packages": {
                f.name: {target(): {"url": f"{base}/{f.name}", "sha256": sha256(f)}}
                for f in files.values()
            }
        }
    ).encode()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            if self.path == "/drivers.json":
                self.reply(manifest)
            elif self.path in files:
                self.reply(files[self.path].read_bytes())
            else:
                self.send_error(404)

        def reply(self, body: bytes) -> None:
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format: str, *args: object) -> None:
            pass

    print(f"drivers: serving {', '.join(sorted(f.name for f in files.values()))} on {base}")
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    # No `match`: the python3 of Xcode's command line tools is 3.9.
    args = sys.argv[1:]
    if len(args) == 3 and args[0] == "collect":
        collect(Path(args[1]), Path(args[2]))
    elif len(args) == 2 and args[0] == "refresh":
        refresh(Path(args[1]))
    elif len(args) == 3 and args[0] == "serve":
        serve(Path(args[1]), int(args[2]))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
