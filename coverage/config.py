"""Per-package coverage settings from coverage/config.yaml."""
import fnmatch
import os
import subprocess

import yaml


def _file():
    """The workspace's coverage/config.yaml; the one next to this script when the
    workspace has none (another repo running the tools from .ci/)."""
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip()
    own = os.path.join(root, "coverage", "config.yaml")
    return own if root and os.path.exists(own) else os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.yaml")


FILE = _file()


def settings(package):
    if not os.path.exists(FILE):
        return {}
    return (yaml.safe_load(open(FILE)) or {}).get(package) or {}


def patterns(package):
    return settings(package).get("exclude") or []


def excluded(package):
    """A predicate on a region's crate-relative path."""
    globs = patterns(package)
    return lambda path: any(fnmatch.fnmatch(path, g) for g in globs)


def tolerance(package):
    return int(settings(package).get("tolerance") or 0)
