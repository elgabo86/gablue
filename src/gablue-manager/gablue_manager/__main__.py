"""Permet `python3 -m gablue_manager` en développement, comme le lanceur."""

import sys

from .app import main

if __name__ == "__main__":
    sys.exit(main())
