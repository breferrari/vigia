#!/bin/sh
# Writes one release's section into CHANGELOG.md, from the labelled pull
# requests in the range being released.
#
# Lives in a file rather than inline in the workflow for the reason
# raise-version.sh does: a filter decides what a user is told about a release,
# and a filter nothing drives is a wish. The test feeds it fixed records and
# reads back the section, so the rules below are checkable without a release.
#
# Records arrive on stdin, newest first, one per commit or per `Release-note:`
# line, as three tab-separated fields:
#
#   who     `#<n>` for the pull request the commit merged, or the short SHA
#           when none was found
#   labels  the pull request's labels, comma-separated, or empty
#   text    the commit subject, or one `Release-note: <text>` line from its
#           body in the subject's place
#
# Intent lives on the label and on the trailer, never in the wording of the
# subject. Per record:
#
#   Release-note: none      skipped. `release` on the PR contradicts it: fail.
#   Release-note: <text>    written as it stands. The PR must carry `release`.
#   subject, `release`      written, minus the tracker's trailing `(#n)`.
#   subject, `internal`     skipped.
#   subject, neither        fail, naming the PR. Unlabelled is an error, not a
#                           guess: the old subject list let factory work
#                           through whenever its wording was ordinary.
#   both labels             fail.
#
# A range that keeps nothing exits 1, so the bump stops before a version is
# cut. Every failure in the range is named before the exit, so one run shows
# every PR that needs a label.
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

# Read once, because the records are walked twice: once to decide, once to
# name the ones in contract phrasing.
records=$(cat)

# One pass decides every record. Kept lines go to stdout prefixed `keep:`, and
# everything else is a workflow annotation. The trailing `(#n)` references are
# peeled here, one at a time, because a merge subject carries the issue's
# number and the pull request's, and sometimes two issues'.
decided=$(printf '%s\n' "$records" | awk '
    BEGIN { FS = "\t"; bad = 0 }
    function strip(s) {
        while (match(s, /[ \t]*\(#[0-9]+(,[ \t]*#[0-9]+)*\)[ \t]*$/)) s = substr(s, 1, RSTART - 1)
        sub(/[ \t]+$/, "", s)
        return s
    }
    NF == 0 { next }
    NF != 3 {
        print "::error::malformed record, expected who<TAB>labels<TAB>text: " $0
        bad = 1
        next
    }
    {
        who = $1; labels = "," $2 ","; text = $3
        release = index(labels, ",release,") > 0
        internal = index(labels, ",internal,") > 0
        if (release && internal) {
            print "::error::" who " is labelled both release and internal"
            bad = 1
            next
        }
        if (text ~ /^Release-note:/) {
            note = text
            sub(/^Release-note:[ \t]*/, "", note)
            sub(/[ \t]+$/, "", note)
            if (tolower(note) == "none") {
                if (release) {
                    print "::error::" who " says Release-note: none and is labelled release"
                    bad = 1
                } else {
                    print "::notice::skipped, Release-note: none: " who
                }
                next
            }
            if (note == "") {
                print "::error::" who " carries an empty Release-note"
                bad = 1
                next
            }
            if (internal) {
                print "::error::" who " is labelled internal and carries Release-note: " note
                bad = 1
                next
            }
            if (!release) {
                print "::error::" who " carries Release-note: " note " and no release label"
                bad = 1
                next
            }
            print "keep:" strip(note)
            next
        }
        if (internal) {
            print "::notice::skipped as internal: " who " " text
            next
        }
        if (!release) {
            print "::error::" who " is labelled neither release nor internal: " text
            bad = 1
            next
        }
        kept = strip(text)
        if (kept != "") print "keep:" kept
    }
    END { exit bad }
') || failed=1
: "${failed:=0}"

printf '%s\n' "$decided" | grep -v '^keep:' | grep -v '^$' || true

if [ "$failed" -ne 0 ]; then
    echo "::error::label every pull request in the range release or internal, and put Release-note: none only on internal ones"
    exit 1
fi

kept=$(printf '%s\n' "$decided" | sed -n 's/^keep://p')

if [ -z "$kept" ]; then
    printf '%s\n' "$records" | awk -F'\t' 'NF == 3 { print "::notice::process only: " $1 " " $3 }'
    echo "::error::nothing user-facing since the last release, so ${version} is not released"
    exit 1
fi

# A kept line in contract phrasing reaches the notes as written. CI rejects
# those on the pull request; this only names any that got through.
printf '%s\n' "$kept" \
    | sh "$(dirname "$0")/title-check.sh" --match \
    | sed 's/^/::warning::contract phrasing in the release notes: /'

entries=$(printf '%s\n' "$kept" | sed -E 's/^/- /')

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
