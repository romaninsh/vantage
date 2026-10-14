"""Per-package coverage settings from coverage/config.yaml."""
import fnmatch
import os

import yaml

FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.yaml")


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
