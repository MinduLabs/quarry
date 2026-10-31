"""Writes the updater's latest.json to stdout: where each package's self-update bundle is and its
signature. Only the macOS app and the AppImage update themselves.

    updater-manifest.py VERSION BASE_URL [--macos SIG] [--appimage SIG]

BASE_URL is where the release's files are downloaded from; each SIG is the `.sig` the build wrote
next to the bundle, named after it.
"""

import argparse
import datetime
import json
import pathlib

parser = argparse.ArgumentParser()
parser.add_argument("version")
parser.add_argument("base_url")
parser.add_argument("--macos", type=pathlib.Path)
parser.add_argument("--appimage", type=pathlib.Path)
args = parser.parse_args()


def entry(sig: pathlib.Path) -> dict:
    return {
        "url": f"{args.base_url.rstrip('/')}/{sig.name.removesuffix('.sig')}",
        "signature": sig.read_text().strip(),
    }


platforms = {}
if args.macos:
    # One universal bundle serves both kinds of Mac.
    platforms["darwin-aarch64"] = entry(args.macos)
    platforms["darwin-x86_64"] = entry(args.macos)
if args.appimage:
    # The installer suffix keeps a .deb, which looks for `linux-x86_64-deb` then
    # `linux-x86_64`, from taking the AppImage.
    platforms["linux-x86_64-appimage"] = entry(args.appimage)
if not platforms:
    raise SystemExit("no signatures given")

print(
    json.dumps(
        {
            "version": args.version,
            "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "platforms": platforms,
        },
        indent=2,
    )
)
