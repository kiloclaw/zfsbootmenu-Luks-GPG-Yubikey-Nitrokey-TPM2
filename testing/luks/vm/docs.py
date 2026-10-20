#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Extracts the shell commands of a section of the reStructuredText documentation,
so that tests run the documented procedures as written.

In a ``.. code-block:: bash`` block, a command starts with ``# `` (the root
prompt); following lines indented deeper than the prompt continue it, lines at
the prompt's indentation are example output and are skipped. A block without
any prompt (a function definition, an array assignment) is one command.
"""

import re
import sys
from pathlib import Path

UNDERLINE = re.compile(r"^([=\-~^\"'`#*+])\1{3,}\s*$")


def sections(path):
    """{title: text} for every section of an RST file."""
    lines = Path(path).read_text().splitlines()
    out, title, buf = {}, None, []
    i = 0
    while i < len(lines):
        if i + 1 < len(lines) and lines[i].strip() and UNDERLINE.match(lines[i + 1]) \
                and len(lines[i + 1].strip()) >= len(lines[i].strip()):
            if title is not None:
                out[title] = "\n".join(buf)
            title, buf = lines[i].strip(), []
            i += 2
            continue
        buf.append(lines[i])
        i += 1
    if title is not None:
        out[title] = "\n".join(buf)
    return out


def blocks(text):
    """Code blocks (list of their lines, dedented) of a section."""
    lines = text.splitlines()
    res = []
    i = 0
    while i < len(lines):
        m = re.match(r"^(\s*)\.\. code-block:: (bash|sh|shell)\s*$", lines[i])
        if not m:
            i += 1
            continue
        base = len(m.group(1))
        i += 1
        block = []
        while i < len(lines):
            line = lines[i]
            if line.strip() and (len(line) - len(line.lstrip())) <= base:
                break
            block.append(line)
            i += 1
        while block and not block[-1].strip():
            block.pop()
        while block and not block[0].strip():
            block.pop(0)
        if block:
            ind = min(len(l) - len(l.lstrip()) for l in block if l.strip())
            res.append([l[ind:] for l in block])
    return res


def commands(path, title):
    """Shell commands of the section with this title, in order."""
    secs = sections(path)
    if title not in secs:
        raise KeyError(f"no section '{title}' in {path}")
    cmds = []
    for block in blocks(secs[title]):
        prompted = [l for l in block if l.startswith("# ")]
        if not prompted:
            cmds.append("\n".join(block))
            continue
        cur = None
        for line in block:
            if line.startswith("# "):
                if cur is not None:
                    cmds.append("\n".join(cur))
                cur = [line[2:]]
            elif cur is not None and line.strip() and line.startswith(" "):
                cur.append(line.strip())
            elif cur is not None:
                cmds.append("\n".join(cur))
                cur = None
        if cur is not None:
            cmds.append("\n".join(cur))
    return cmds


if __name__ == "__main__":
    for c in commands(sys.argv[1], sys.argv[2]):
        print("----\n" + c)
