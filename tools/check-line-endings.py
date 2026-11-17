#!/usr/bin/env python3
"""Refuse CRLF in tracked text files, because a line ending is part of a hash here.

WHY THIS REPOSITORY CARES MORE THAN MOST. `ModelHash` is computed over the raw
bytes of a model pack, deliberately, so that a parser change cannot
retroactively alter a stored hash. The consequence is that the CRLF and LF
versions of the same pack are **different models**. A pack that drifts to CRLF
gets a different `ModelHash`, every committed record referencing it fails
replay, and on Windows the file looks identical in every editor.

`.gitattributes` already pins `* text=auto eol=lf` and every tracked file is
`i/lf w/lf` today. That is not enough on its own, and the gap is specific:

  ** A CLEAN INDEX IS NOT A CLEAN WORKING TREE. **

`.gitattributes` normalises on checkout and on `git add`, so the index stays
LF by itself. What it does not cover is a Windows tool rewriting a file **in
place** with CRLF. The index stays LF, the working tree drifts, and
`git status` is silent because the filter reconciles them on the way past.

That drift is invisible until the working tree leaves git. A build or
experiment run from a tree copied to another machine (for example
`tar -cf - ... | ssh host`) uses the working tree, not a commit. A pack that
drifted to CRLF locally would be hashed as a different model on the build
machine, and the failure would arrive as a replay mismatch with no obvious
cause.

THE MEASUREMENT TRAP — read this before "verifying" anything here.
On Windows the obvious ways to count carriage returns lie:

  * `grep -c $'\\r' <file>` and anything piped through MSYS or Git Bash can ADD
    CR to the stream, so pure-LF files are reported as CRLF.
  * `git show` / `git cat-file` piped into another command do the same.
    `core.autocrlf` is `true` on the authoring box; `.gitattributes` makes it
    inert for tracked files, but it is still what makes those commands emit
    translated output that has nothing to do with the bytes on disk.

`git ls-files --eol` is the authoritative diagnostic, and it stays
authoritative through a pipe: the `i/` and `w/` fields are git's own verdict
computed from the real bytes, so whatever a transport does to the report
cannot change what the report says. This script drives off that rather than
re-implementing text detection — re-implementing would also mean
re-implementing `.gitattributes` resolution, and getting that subtly wrong is
how a checker starts lying too. Anything here touching file content opens
`'rb'`. Never shell out to grep.

Usage:
  python tools/check-line-endings.py           # every tracked text file; exit 1 on CRLF
  python tools/check-line-endings.py --staged  # only staged files (the pre-commit hook)
  python tools/check-line-endings.py --fix     # rewrite drifted working-tree files to LF

ESCAPE HATCH: a file that genuinely must keep CRLF is declared in
`.gitattributes` as `-text` or `eol=crlf`, with a reason on the line above.
This script honours both. Nothing in this repository needs it today.
"""

import subprocess
import sys

# Attributes meaning "do not touch my line endings". Exact tokens, so
# `text=auto` never accidentally matches `-text`.
EXEMPT_ATTRS = frozenset(("-text", "eol=crlf"))


def git(*args):
    """Run a git command and return stdout as bytes."""
    return subprocess.run(
        ["git", *args], check=True, stdout=subprocess.PIPE
    ).stdout


def tracked_eol(staged_only):
    """[(path, index_eol, worktree_eol, attrs)] for every tracked text file.

    Parses `git ls-files -z --eol`. Each NUL-separated record looks like:

        i/lf    w/crlf  attr/text=auto eol=lf \tpath/with spaces.md

    The eol block and the path are separated by a single TAB. `-z` is not
    cosmetic: without it git C-quotes any path containing a tab, a quote or a
    non-ASCII byte, and the TAB split would then land in the wrong place.
    """
    argv = ["ls-files", "-z", "--eol"]
    if staged_only:
        # Restrict to what is actually being committed. `--cached` is the
        # index, which is what a pre-commit hook is deciding about.
        staged = git("diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR")
        paths = [p for p in staged.decode("utf-8", "replace").split("\0") if p]
        if not paths:
            return []
        argv += ["--"] + paths

    records = git(*argv).decode("utf-8", "replace").split("\0")

    found = []
    for record in records:
        if not record or "\t" not in record:
            continue
        info, path = record.split("\t", 1)
        fields = info.split()
        index_eol = worktree_eol = ""
        attrs = ""
        for field in fields:
            if field.startswith("i/"):
                index_eol = field[2:]
            elif field.startswith("w/"):
                worktree_eol = field[2:]
            elif field.startswith("attr/"):
                attrs = field[5:]
        found.append((path, index_eol, worktree_eol, attrs))
    return found


def is_exempt(attrs):
    """Whether `.gitattributes` says this file's endings are deliberate."""
    return any(token in EXEMPT_ATTRS for token in attrs.split())


def looks_binary(path):
    """Whether this is a real binary rather than text carrying a stray byte.

    A NUL is the honest tell. Source and YAML never contain one.

    ⛔ CORRECTED 2026-08-16: this read only the first 8192 bytes, on the
    reasoning that real binaries carry a NUL "in the first few KiB". That is
    usually true and is not what git does, so the two disagreed at the edge --
    and the disagreement produced a FALSE POSITIVE, which is the expensive
    direction for a commit gate.

    MEASURED, not reasoned: `git ls-files --eol` classifies from stats over the
    WHOLE blob, so a file whose only NUL sits at byte 9000 is still reported
    `-text`. A genuine binary with a CR at byte 50 and its first NUL at byte
    9000 was therefore called binary by git, not recognised as binary here, and
    reported by this gate as a corrupted text file -- this checker calling git
    a liar about git's own verdict.

    The 8000-ish figure is real but belongs to the DIFF machinery
    (`buffer_is_binary`'s FIRST_FEW_BYTES), not to eol classification: a true
    constant borrowed from the wrong subsystem.
    """
    try:
        with open(path, "rb") as handle:
            return b"\x00" in handle.read()
    except OSError:
        return True  # unreadable: not ours to judge


def stray_cr(path):
    """A carriage return that is not part of a CRLF line ending.

    THE GAP THIS CLOSES, and it was in this very file. A CR *inside* a line is
    not a line-ending problem, so `git ls-files --eol` does not report `crlf`
    or `mixed` — it reclassifies the file as `-text`, i.e. **binary**. The
    check above then treats `-text` as a deliberate exemption and skips it
    entirely, reporting "all LF": true, and useless.

    That is not hypothetical. A shell escape `$'\r'` written as a real CR byte
    instead of backslash-r, then round-tripped through a Python read WITHOUT
    `newline=''`, silently became `$'\n'` — universal newlines translate a
    lone CR. The strip it performed stopped stripping, and a header kept its
    carriage return.

    Every checker that trusts `git ls-files --eol` would have passed that file.
    """
    try:
        with open(path, "rb") as handle:
            raw = handle.read()
    except OSError:
        return 0
    # Remove well-formed CRLF first; whatever CR remains is loose.
    return raw.replace(b"\r\n", b"").count(b"\r")


def to_lf(path):
    """Rewrite a working-tree file to LF. Bytes in, bytes out."""
    with open(path, "rb") as handle:
        raw = handle.read()
    # Normalise CRLF and lone CR, in that order, so a file with both does not
    # end up with doubled newlines.
    fixed = raw.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
    if fixed == raw:
        return False
    with open(path, "wb") as handle:
        handle.write(fixed)
    return True


def main():
    staged_only = "--staged" in sys.argv
    fix = "--fix" in sys.argv

    try:
        entries = tracked_eol(staged_only)
    except subprocess.CalledProcessError as error:
        print(f"check-line-endings: git failed ({error})", file=sys.stderr)
        return 2

    drifted = []
    corrupt = []
    for path, index_eol, worktree_eol, attrs in entries:
        # A stray CR is checked BEFORE the exemption, because that is exactly
        # what the exemption hides: git calls such a file `-text`, and `-text`
        # is one of the tokens that means "leave my endings alone".
        if not looks_binary(path):
            loose = stray_cr(path)
            if loose:
                corrupt.append((path, loose))
                continue

        if is_exempt(attrs):
            continue
        # `none` means git found no line endings at all (empty or single
        # line); `mixed` and `crlf` are both wrong here.
        if index_eol in ("crlf", "mixed") or worktree_eol in ("crlf", "mixed"):
            drifted.append((path, index_eol, worktree_eol))

    if corrupt:
        print("check-line-endings: stray carriage return inside a text file.",
              file=sys.stderr)
        print("", file=sys.stderr)
        for path, loose in corrupt:
            print(f"  {loose} loose CR  {path}", file=sys.stderr)
        print(
            "\nA CR *inside* a line is not a line-ending problem, so git calls the file\n"
            "binary (`-text`) and every endings check skips it. It is usually a Python\n"
            "round-trip that read without `newline=''`, which turns a lone CR into a\n"
            "newline and silently rewrites whatever the byte meant -- a shell `$'\\r'`\n"
            "escape, most often. Fix the byte, and read with `newline=''` on both ends.\n",
            file=sys.stderr,
        )
        return 1

    if not drifted:
        scope = "staged" if staged_only else "tracked"
        print(f"check-line-endings: {len(entries)} {scope} files, all LF, no stray CR")
        return 0

    if fix:
        healed = 0
        for path, _, _ in drifted:
            try:
                if to_lf(path):
                    healed += 1
                    print(f"  fixed  {path}")
            except OSError as error:
                print(f"  FAILED {path}: {error}", file=sys.stderr)
        print(f"check-line-endings: rewrote {healed} file(s) to LF")
        print("Re-stage them: git add -u")
        return 0

    print("check-line-endings: CRLF found in tracked text files.", file=sys.stderr)
    print("", file=sys.stderr)
    for path, index_eol, worktree_eol in drifted:
        print(f"  i/{index_eol:<5} w/{worktree_eol:<5} {path}", file=sys.stderr)
    print("", file=sys.stderr)
    print(
        "A line ending is part of `ModelHash` here: the CRLF and LF versions of a\n"
        "pack are different models, and every committed record referencing one of\n"
        "them would fail replay. Heal it with:\n"
        "\n"
        "    python tools/check-line-endings.py --fix && git add -u\n",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
