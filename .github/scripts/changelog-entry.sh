#!/bin/sh
# Writes one release's section into CHANGELOG.md, from the commit subjects of
# the range being released.
#
# Lives in a file rather than inline in the workflow for the reason
# raise-version.sh does: a filter decides what a user is told about a release,
# and a filter nothing drives is a wish. The test feeds it fixed subjects and
# reads back the section, so the rules below are checkable without a release.
#
# Subjects arrive on stdin, one per line, newest first, which is what
# `git log --format=%s` gives and the order the section is written in.
#
# Usage: changelog-entry.sh <version> <date> [changelog]
#   version   the version being released, three numeric components
#   date      ISO date for the heading
#   changelog the file to write into (default: ./CHANGELOG.md)
set -eu

[ "$#" -ge 2 ] || { echo "::error::usage: changelog-entry.sh <version> <date> [changelog]"; exit 1; }
version="$1"
date="$2"
changelog="${3:-CHANGELOG.md}"

case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "::error::${version} is not three numeric components"; exit 1 ;;
esac

[ -f "$changelog" ] || { echo "::error::${changelog} does not exist"; exit 1; }

# A section for this version already existing means the release is being
# re-run, or that somebody wrote it by hand. Either way, overwriting it would
# discard the better text, so this stops instead.
if grep -qF "## [${version}]" "$changelog"; then
    echo "::error::${changelog} already carries a section for ${version}"
    exit 1
fi

# **The two filters are different rules and neither subsumes the other.**
#
# The first drops the repository's own prefix convention: a subject beginning
# `roadmap:` or `spec:` is work on the written layer, which a user of the binary
# cannot observe. `feat:` and `fix:` are deliberately absent, because those
# describe the product and predate the prose-title convention.
#
# The second drops the subjects that describe the same work without a prefix,
# which is most of them: a row moving on the roadmap, a phase reordering, a
# ruling being written, a token the release needed. It matches anywhere in the
# subject rather than at an anchor, because the internal thing is as often the
# object as the subject, and case-insensitively, because `ROADMAP.md` and `the
# roadmap` are the same subject twice.
#
# **Scoping the range to `crates/` instead was tried and does not work here.**
# It is the usual way to do this and it looks stricter, but this repository
# gates its own prose from tests, so a commit that only edits ROADMAP.md still
# touches `crates/vigia/tests/package.rs` to move a ceiling. Measured over
# 0.34.0 to 0.38.0, the path scope kept every document commit in the range.
#
# The list is tuned against the 143 subjects this repository had at 0.38.0 and
# is a heuristic, not a rule: it will pass something internal through, and the
# cost of that is one odd line in a release note rather than a broken release.
#
# `token` is bounded the long way round rather than with `\b`, which is a GNU
# extension that POSIX ERE does not define. The release runs this on one runner
# whose grep has it, but the test that drives the filter is `cfg(unix)` and so
# runs wherever the suite does. Unbounded, the word would swallow `tokenizer`,
# which in a syntax highlighter is a subject a reader can see.
internal_prefix='^(roadmap|spec|docs?|ci|chore|deps?|take-next|skill|test|refactor|perf|style|build|process|steering|mockup|harden|release|vault writes)(\([^)]*\))?: '
internal_subject='(roadmap|spec\.md|the spec|rulings?|revocation|withdrawn|written layer|phase [0-9]|the shelf|shelved|readme|claude\.md|clippy|cargo doc|ci complete|workflow|pre-flight|version raise|release note|the release |the bump |(^|[^a-zA-Z])tokens?([^a-zA-Z]|$)|take-next|the skill|the harness|the record|budget table|mutation|audit|ceiling|proposal|declined|adopted|review agent|assertion|the mockup|funding|\.yml|§|^track the |^#[0-9]|^b[0-9]+[ :]|^[0-9]+\.[0-9]+,)'

# `|| true` on both greps, because grep exits 1 when it filters everything out
# and `set -e` would kill the script on the assignment rather than let the
# empty case below be handled.
kept=$(grep -Ev "$internal_prefix" || true)
kept=$(printf '%s\n' "$kept" | grep -Eiv "$internal_subject" || true)

# Trailing `(#123)` references are the tracker's, not the reader's. Stripped
# repeatedly because a merge subject carries the issue's number and the pull
# request's, and sometimes two issues'.
entries=$(printf '%s\n' "$kept" \
    | sed -E 's/[[:space:]]*\((#[0-9]+(,[[:space:]]*#[0-9]+)*)\)[[:space:]]*$//' \
    | sed -E 's/[[:space:]]*\((#[0-9]+(,[[:space:]]*#[0-9]+)*)\)[[:space:]]*$//' \
    | sed -E 's/[[:space:]]*\((#[0-9]+(,[[:space:]]*#[0-9]+)*)\)[[:space:]]*$//' \
    | sed -E 's/[[:space:]]+$//' \
    | grep -v '^$' \
    | sed -E 's/^/- /' \
    || true)

# **An empty section is written rather than skipped, and it says so.** A version
# with no section makes `dist` fall back to the install instructions alone, with
# no warning anywhere, so the release that had nothing to report and the release
# whose notes were lost look identical to a reader. This tells them apart.
if [ -z "$entries" ]; then
    entries="- Internal changes only. Nothing a user of the pane can see moved."
fi

# Written above the newest existing section, so the file stays newest first.
# `awk` rather than `sed`, because the insertion is a block and the anchor is
# the first line of many that match.
#
# The entries reach awk through a file rather than through `-v`, which
# interprets backslash escapes in the value it is given: a commit subject
# carrying one would arrive mangled, and a subject ending in one would eat the
# line after it.
lines="${changelog}.entries"
printf '%s\n' "$entries" > "$lines"

inserted="${changelog}.inserted"
awk -v heading="## [${version}] - ${date}" -v lines="$lines" '
    function emit() {
        print heading
        print ""
        while ((getline entry < lines) > 0) print entry
        close(lines)
    }
    !written && /^## \[/ {
        emit()
        print ""
        written = 1
    }
    { print }
    END { if (!written) emit() }
' "$changelog" > "$inserted"
mv "$inserted" "$changelog"
rm -f "$lines"

# Verify the write rather than trusting it, on raise-version.sh's rule: an awk
# that matched nothing exits 0 and leaves the file as it was.
if ! grep -qF "## [${version}] - ${date}" "$changelog"; then
    echo "::error::the section for ${version} was not written to ${changelog}"
    exit 1
fi

echo "::notice::wrote $(printf '%s\n' "$entries" | wc -l | tr -d ' ') entry line(s) for ${version}"
