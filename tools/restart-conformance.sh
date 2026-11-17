#!/usr/bin/env bash
#
# Machine-restart conformance, from plan section 21.
#
# The one attack on that list that cannot be a `cargo test`, because a test
# cannot reboot the host it is running on. It was a manual procedure in
# docs/verification.md; this is the same procedure with the human taken out
# of the loop, so it can be run on a schedule and cannot be run wrongly.
#
#   ./tools/restart-conformance.sh arm --reboot
#   ...machine reboots, the check runs itself on the way back up...
#   ./tools/restart-conformance.sh status
#
# ---------------------------------------------------------------------
# What makes this a real test rather than a second run
# ---------------------------------------------------------------------
#
# Nothing about "run it, reboot, run it again" is self-evidently a restart
# test. If the reboot silently failed, the verify stage would pass exactly
# as it does now and report success, and the whole exercise would be a
# slower way of running the suite twice.
#
# So the baseline records the kernel's boot ID, which is regenerated on
# every boot and cannot be forged from userspace. The verify stage
# **refuses to report a pass** unless that ID has changed. The test proves
# the machine restarted before it proves anything about the engine.
#
# ---------------------------------------------------------------------
# Why cron rather than a systemd unit
# ---------------------------------------------------------------------
#
# A user crontab needs no root, no unit file, and no lingering. `arm`
# writes one `@reboot` line and `verify` removes it as its very first act,
# before doing anything that could fail. A reboot loop is the one failure
# mode that would matter here, and removing the hook first makes it
# impossible: even if every subsequent line crashes, nothing reboots and
# nothing is left armed.
#
# The hook is tagged with a marker comment so it can be removed without
# disturbing anything else in the user's crontab.

set -euo pipefail

readonly MARKER="dre-restart-conformance"

# Overridable so `selftest` can exercise the archive against a throwaway
# directory. A test that appended to the real history to prove appending
# works would be writing fiction into the evidence it is meant to protect.
STATE_DIR="${DRE_CONFORMANCE_STATE_DIR:-${HOME}/.local/state/dre-restart-conformance}"
readonly STATE_DIR
readonly BASELINE="${STATE_DIR}/baseline"
readonly REPORT="${STATE_DIR}/report"
readonly LOG="${STATE_DIR}/verify.log"

# The recurring schedule carries its own marker, so disarming one cycle does
# not cancel the schedule -- the check would stop running and nothing would
# say so, which is the failure mode a scheduled check exists to avoid.
#
# A separate marker is necessary and not sufficient: this one has the other
# as a prefix, so the removal has to match the whole tag. See
# `drop_marked_lines`.
readonly SCHEDULE_MARKER="dre-restart-conformance-schedule"
readonly HISTORY="${STATE_DIR}/history"
readonly CYCLE_LOG="${STATE_DIR}/cycle.log"

readonly EXPERIMENTS=(
	logic-001
	diagnosis-001
	diagnosis-002
	navigation-001
	process-001
	process-002
)

# Resolved once so the crontab entry survives a different working
# directory on the way back up.
SCRIPT="$(readlink -f "${BASH_SOURCE[0]}")"
readonly SCRIPT
REPO="$(dirname "$(dirname "${SCRIPT}")")"
readonly REPO

boot_id() {
	cat /proc/sys/kernel/random/boot_id
}

# Cargo is not on PATH in a cron environment, which is exactly the kind of
# difference between an interactive run and an automated one that this
# script exists to survive.
load_toolchain() {
	# shellcheck disable=SC1091
	[ -f "${HOME}/.cargo/env" ] && . "${HOME}/.cargo/env"
	return 0
}

# One line per experiment: `<id> <decision> <proof>`.
#
# Taken from `dre replay`, so this also asserts each experiment still
# reproduces its committed record. A baseline captured from a run that was
# already failing would compare a broken state against itself and pass.
capture_hashes() {
	local experiment decision proof output
	for experiment in "${EXPERIMENTS[@]}"; do
		if ! output="$(cargo run --release --quiet --bin dre -- \
			replay "experiments/${experiment}.yaml" 2>&1)"; then
			echo "FATAL: ${experiment} does not currently reproduce its baseline" >&2
			echo "${output}" >&2
			return 1
		fi
		decision="$(printf '%s\n' "${output}" | awk '/^Decision hash:/ {print $3}')"
		proof="$(printf '%s\n' "${output}" | awk '/^Proof hash:/ {print $3}')"
		if [ -z "${decision}" ] || [ -z "${proof}" ]; then
			echo "FATAL: could not read hashes for ${experiment}" >&2
			return 1
		fi
		printf '%s %s %s\n' "${experiment}" "${decision}" "${proof}"
	done
}

# Replace this marker's crontab line, leaving every other line alone.
#
# Anchored for the same reason `drop_marked_lines` is, and missing it here
# was worse: installing the boot hook filtered on the bare marker, which is
# a prefix of the schedule's, so arming a cycle deleted the schedule. The
# check would have run once and then never again, and the crontab would
# have looked deliberate.
#
# Fixing removal and not installation is how that survived one round of
# testing -- `schedule` then `disarm` exercises only the removal half.
install_marked_line() {
	local marker="$1" entry="$2"
	{
		crontab -l 2>/dev/null | grep -v -- "# ${marker}\$" || true
		printf '%s # %s\n' "${entry}" "${marker}"
	} | crontab -
}

install_hook() {
	install_marked_line "${MARKER}" "@reboot ${SCRIPT} verify >> ${LOG} 2>&1"
}

# Drop the crontab lines tagged with exactly `marker`.
#
# Anchored to the end of the line, and that is load-bearing rather than
# tidy: `dre-restart-conformance` is a prefix of
# `dre-restart-conformance-schedule`, so an unanchored `grep -v` on the
# boot marker deletes the schedule too. Disarming one cycle would silently
# stop the check running ever again -- and it would look like it had
# worked, because disarming is supposed to remove a crontab line.
#
# Caught by testing the two commands against each other rather than by
# reading either one. The comment on SCHEDULE_MARKER claimed the separate
# marker was sufficient; it was not.
drop_marked_lines() {
	local marker="$1" remaining
	remaining="$(crontab -l 2>/dev/null | grep -v -- "# ${marker}\$" || true)"
	if [ -z "${remaining}" ]; then
		crontab -r 2>/dev/null || true
	else
		printf '%s\n' "${remaining}" | crontab -
	fi
}

remove_hook() {
	drop_marked_lines "${MARKER}"
}

cmd_arm() {
	local reboot_now="${1:-no}"

	cd "${REPO}"
	load_toolchain

	# The baseline has to correspond to committed code, or a pass says
	# only that two runs of an unknown tree agreed.
	if [ -n "$(git status --porcelain)" ]; then
		echo "refusing to arm: the working tree has uncommitted changes" >&2
		echo "a baseline that does not correspond to a commit proves nothing" >&2
		return 1
	fi

	mkdir -p "${STATE_DIR}"
	rm -f "${REPORT}"

	echo "Building..."
	cargo build --release --quiet --bin dre

	echo "Capturing baseline..."
	local hashes
	hashes="$(capture_hashes)"

	{
		printf 'commit %s\n' "$(git rev-parse HEAD)"
		printf 'boot_id %s\n' "$(boot_id)"
		printf 'armed_at %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
		printf '%s\n' "${hashes}"
	} >"${BASELINE}"

	install_hook

	echo
	echo "Armed. Baseline: ${BASELINE}"
	sed 's/^/  /' "${BASELINE}"
	echo
	echo "The check will run automatically on the next boot."

	if [ "${reboot_now}" = "yes" ]; then
		echo "Rebooting now..."
		sudo systemctl reboot

		# `systemctl reboot` returns as soon as the request is queued, and
		# the machine then takes several seconds to go down. Returning here
		# hands control back to the caller *while still running*, so
		# anything chained after this executes on the way out.
		#
		# That is not theoretical. Chaining `disarm` after a `cycle` removed
		# the boot hook microseconds after it was installed; the machine
		# rebooted with nothing armed, no verify ran, and the only evidence
		# was a baseline with no report next to it. Nothing failed loudly.
		#
		# So block until the kernel kills us. The reboot is the exit.
		sleep 300
		echo "still alive after 5 minutes -- the reboot did not happen" >&2
		return 1
	fi

	echo "Reboot when ready, or re-run with --reboot."
}

cmd_verify() {
	# First, unconditionally. Everything below can fail without leaving
	# the machine armed to do this again.
	remove_hook

	cd "${REPO}"
	load_toolchain

	if [ ! -f "${BASELINE}" ]; then
		echo "no baseline: run \`${SCRIPT} arm\` first" >&2
		return 1
	fi

	local armed_boot armed_commit now_boot now_commit
	armed_boot="$(awk '/^boot_id/ {print $2}' "${BASELINE}")"
	armed_commit="$(awk '/^commit/ {print $2}' "${BASELINE}")"
	now_boot="$(boot_id)"
	now_commit="$(git rev-parse HEAD)"

	local verdict="PASS"
	local -a notes=()

	# The check that makes this a restart test at all.
	if [ "${armed_boot}" = "${now_boot}" ]; then
		verdict="INCONCLUSIVE"
		notes+=("the boot ID is unchanged, so the machine did not restart")
	fi

	if [ "${armed_commit}" != "${now_commit}" ]; then
		verdict="INCONCLUSIVE"
		notes+=("HEAD moved from ${armed_commit} to ${now_commit}; different code, nothing proven")
	fi

	cargo build --release --quiet --bin dre

	local fresh
	if ! fresh="$(capture_hashes)"; then
		verdict="FAIL"
		notes+=("at least one experiment no longer reproduces its committed record")
		fresh=""
	fi

	# Compare only the hash lines; the header lines are metadata.
	local expected
	expected="$(grep -E '^[a-z]+-[0-9]+ ' "${BASELINE}" || true)"

	if [ -n "${fresh}" ] && [ "${fresh}" != "${expected}" ]; then
		verdict="FAIL"
		notes+=("hashes differ across the restart")
	fi

	{
		printf 'verdict %s\n' "${verdict}"
		printf 'commit %s\n' "${now_commit}"
		printf 'boot_before %s\n' "${armed_boot}"
		printf 'boot_after %s\n' "${now_boot}"
		printf 'armed_at %s\n' "$(awk '/^armed_at/ {print $2}' "${BASELINE}")"
		printf 'verified_at %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
		printf 'experiments %s\n' "${#EXPERIMENTS[@]}"
		local note
		for note in ${notes+"${notes[@]}"}; do
			printf 'note %s\n' "${note}"
		done
		printf '%s\n' "${fresh}"
	} >"${REPORT}"

	# Folded into the history here, as soon as the verdict exists, rather
	# than by the next `cycle`.
	#
	# It used to be the next cycle's first act, which meant `history` was
	# always one cycle behind `status`: the check would run, pass, and the
	# pass rate would not mention it until the following week. Reading the
	# two side by side showed "4 of 5 cycles passed" next to a PASS that was
	# not among the five, which invites exactly the wrong conclusion about
	# the most recent run.
	#
	# Before `cat`, so the report shown includes its own archive stamp, and
	# before the verdict check, so a FAIL is recorded rather than lost to
	# `set -e` on the way out. A failing cycle is the one most worth having
	# in the history.
	archive_report

	cat "${REPORT}"
	[ "${verdict}" = "PASS" ]
}

cmd_status() {
	if [ ! -f "${REPORT}" ]; then
		echo "no report yet"
		if crontab -l 2>/dev/null | grep -q "${MARKER}"; then
			echo "armed: the check will run on the next boot"
		else
			echo "not armed"
		fi
		return 0
	fi
	cat "${REPORT}"
	grep -q '^verdict PASS' "${REPORT}"
}

cmd_disarm() {
	remove_hook
	echo "disarmed"
}

# ---------------------------------------------------------------------
# Running it unattended
# ---------------------------------------------------------------------
#
# `arm` is a single shot: it records a baseline, reboots, and the boot hook
# verifies once. Someone still has to start each one. On a machine whose
# only job is this check, that someone should be cron.
#
# Two things have to be true before a schedule is honest, and neither is
# true of `arm` alone.
#
# The first is that `verify` **overwrites** the report. Run on a schedule,
# each cycle would erase the evidence of the one before it, so a failure
# last Tuesday is gone by Wednesday and the only readable state is the most
# recent run. Folding each report into a history turns a single
# latest-result file into a record.
#
# `verify` does that folding itself, as soon as it has a verdict. Archiving
# used to be the next cycle's first act, which left `history` permanently
# one cycle behind `status` — a check could run, pass, and not appear in its
# own pass rate until the following week.
#
# The second is that a scheduled check must not quietly stop. A cycle that
# fails is louder than one that never ran, so the archive records skipped
# and failed cycles explicitly rather than leaving a gap to be noticed. That
# case is still the next cycle's job, because a `verify` that never ran
# cannot report on itself.

install_schedule() {
	local spec="$1"
	install_marked_line "${SCHEDULE_MARKER}" "${spec} ${SCRIPT} cycle >> ${CYCLE_LOG} 2>&1"
}

remove_schedule() {
	drop_marked_lines "${SCHEDULE_MARKER}"
}

# Fold the current report into the history, whatever it says.
#
# Idempotent, and it has to be: `verify` calls this the moment it has a
# verdict, `cycle` calls it to catch the cases `verify` never reached, and
# `archive` exists so a report written by an older copy of this script can
# be folded in by hand. Three callers, one report, and a second append would
# inflate the pass rate — which is the one number here anybody acts on.
#
# The stamp lives in the report rather than in a marker file beside it, so
# there is exactly one thing to read and no way for the two to disagree
# about whether a cycle was recorded.
archive_report() {
	mkdir -p "${STATE_DIR}"

	if [ -f "${REPORT}" ] && grep -q '^archived_at ' "${REPORT}"; then
		return 0
	fi

	if [ ! -f "${REPORT}" ]; then
		# Distinguish "the previous cycle produced nothing" from "there was
		# no previous cycle". The first means the machine armed and never
		# came back to verify, which is a real failure and the one a
		# scheduled check is most likely to hit.
		if [ -f "${BASELINE}" ]; then
			printf '%s cycle=incomplete commit=%s note=armed-but-never-verified\n' \
				"$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
				"$(awk '/^commit/ {print $2}' "${BASELINE}")" >>"${HISTORY}"
		fi
		return 0
	fi

	printf '%s cycle=%s commit=%s armed=%s verified=%s\n' \
		"$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
		"$(awk '/^verdict/ {print $2}' "${REPORT}")" \
		"$(awk '/^commit/ {print $2}' "${REPORT}")" \
		"$(awk '/^armed_at/ {print $2}' "${REPORT}")" \
		"$(awk '/^verified_at/ {print $2}' "${REPORT}")" >>"${HISTORY}"

	printf 'archived_at %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"${REPORT}"
}

# Still called here, and no longer the thing that keeps the history current.
#
# What it catches now is the case `verify` cannot report on, because it
# never ran: a cycle that armed, rebooted, and never came back. That is a
# real failure and the one a scheduled check is most likely to hit, so it
# has to be noticed by the *next* cycle or not at all.
#
# It also folds in a report left unarchived by an older copy of this script,
# which is why it stayed a call to the same function rather than becoming a
# narrower one.
cmd_cycle() {
	archive_report
	cmd_arm yes
}

cmd_archive() {
	archive_report
	cmd_history
}

cmd_schedule() {
	local spec="${1:-}"
	if [ -z "${spec}" ]; then
		echo "usage: ${0##*/} schedule '<cron expression>'" >&2
		echo "  e.g. ${0##*/} schedule '0 3 * * 0'   # 03:00 every Sunday" >&2
		return 2
	fi

	mkdir -p "${STATE_DIR}"
	install_schedule "${spec}"

	echo "scheduled: ${spec}"
	echo "each firing archives the last report, re-arms, and reboots."
	crontab -l 2>/dev/null | grep "${SCHEDULE_MARKER}" | sed 's/^/  /'
}

cmd_unschedule() {
	remove_schedule
	echo "unscheduled"
}

# Where `selftest_archive` keeps its throwaway state directory.
#
# Global rather than local so the EXIT trap can still read it. A `RETURN`
# trap over a `local` looked tidier and was wrong: the variable is out of
# scope by the time the trap fires, so cleanup died on `set -u` *after*
# every check had already reported ok. A passing selftest that exits
# non-zero is worse than either outcome on its own.
PROBE_DIR=""

# Check the history stays current and stays honest.
#
# Run against a throwaway `STATE_DIR` by re-executing this script, because
# the alternative is a test that appends to the real history to prove
# appending works — and the pass rate is the one number here anybody acts
# on, so writing fiction into it to test it is not a trade worth making.
selftest_archive() {
	local line_count
	PROBE_DIR="$(mktemp -d)"
	trap 'rm -rf "${PROBE_DIR}"' EXIT

	probe() {
		DRE_CONFORMANCE_STATE_DIR="${PROBE_DIR}" "${SCRIPT}" archive >/dev/null 2>&1 || true
	}
	cycles() { grep -c 'cycle=' "${PROBE_DIR}/history" 2>/dev/null || echo 0; }

	# Nothing armed and nothing verified is not a cycle, and must not be
	# recorded as one.
	probe
	if [ "$(cycles)" = "0" ]; then
		report ok 'an empty state records no cycle'
	else
		report fail 'an empty state records no cycle'
	fi

	# A verdict is folded in as soon as it exists.
	printf 'verdict PASS\ncommit abc123\narmed_at t1\nverified_at t2\n' >"${PROBE_DIR}/report"
	probe
	if [ "$(cycles)" = "1" ] && grep -q 'cycle=PASS' "${PROBE_DIR}/history"; then
		report ok 'a fresh report is archived'
	else
		report fail 'a fresh report is archived'
	fi

	# And exactly once. Three callers reach this function; a second append
	# would inflate the pass rate.
	probe
	probe
	if [ "$(cycles)" = "1" ]; then
		report ok 'archiving twice records one cycle'
	else
		report fail 'archiving twice records one cycle'
	fi

	# Armed, rebooted, never came back. The failure a scheduled check is
	# most likely to hit, and the one `verify` cannot report on.
	rm -f "${PROBE_DIR}/report"
	printf 'commit def456\nboot_id x\narmed_at t3\n' >"${PROBE_DIR}/baseline"
	probe
	line_count="$(cycles)"
	if [ "${line_count}" = "2" ] && grep -q 'cycle=incomplete' "${PROBE_DIR}/history"; then
		report ok 'armed-but-never-verified is recorded'
	else
		report fail 'armed-but-never-verified is recorded'
	fi

	rm -rf "${PROBE_DIR}"
	trap - EXIT
}

# Exercise every combination of installing and removing the two crontab
# entries, because this is where both of this script's real bugs lived.
#
# One marker is a prefix of the other, so any unanchored match touches both
# lines. That went wrong twice: first in removal, where disarming a cycle
# deleted the schedule, and then in installation, where arming one deleted
# it again. Neither failed loudly. Both left a crontab that looked like
# somebody had meant it.
#
# The first fix survived a round of testing that only covered removal,
# which is the argument for testing the matrix rather than the path you
# just changed.
cmd_selftest() {
	local saved failures=0
	saved="$(crontab -l 2>/dev/null || true)"

	restore() {
		if [ -z "${saved}" ]; then
			crontab -r 2>/dev/null || true
		else
			printf '%s\n' "${saved}" | crontab -
		fi
	}
	trap restore EXIT

	has() { crontab -l 2>/dev/null | grep -q -- "# $1\$"; }

	report() {
		local outcome="$1" what="$2"
		if [ "${outcome}" = "ok" ]; then
			printf '  ok    %s\n' "${what}"
		else
			printf '  FAIL  %s\n' "${what}"
			failures=$((failures + 1))
		fi
	}

	expect_present() {
		if has "$1"; then report ok "$2"; else report fail "$2"; fi
	}

	expect_absent() {
		if has "$1"; then report fail "$2"; else report ok "$2"; fi
	}

	crontab -r 2>/dev/null || true

	install_schedule '0 3 * * 0'
	expect_present "${SCHEDULE_MARKER}" 'schedule installs'

	install_hook
	expect_present "${MARKER}" 'boot hook installs'
	expect_present "${SCHEDULE_MARKER}" 'installing the boot hook keeps the schedule'

	install_schedule '0 4 * * 0'
	expect_present "${MARKER}" 'installing the schedule keeps the boot hook'

	remove_hook
	expect_present "${SCHEDULE_MARKER}" 'disarm keeps the schedule'
	expect_absent "${MARKER}" 'disarm removes the boot hook'

	install_hook
	remove_schedule
	expect_present "${MARKER}" 'unschedule keeps the boot hook'
	expect_absent "${SCHEDULE_MARKER}" 'unschedule removes the schedule'

	remove_hook
	if [ -z "$(crontab -l 2>/dev/null || true)" ]; then
		report ok 'both removed leaves nothing'
	else
		report fail 'both removed leaves nothing'
	fi

	restore
	trap - EXIT

	selftest_archive

	echo
	if [ "${failures}" -eq 0 ]; then
		echo "selftest: all checks passed"
		return 0
	fi
	echo "selftest: ${failures} check(s) failed" >&2
	return 1
}

cmd_history() {
	if [ ! -f "${HISTORY}" ]; then
		echo "no cycles recorded yet"
		return 0
	fi
	cat "${HISTORY}"

	# A pass rate is the reason to keep a history at all: one green cycle
	# says the machine restarted cleanly once, and the interesting question
	# is whether it always does.
	local total passed
	total="$(grep -c 'cycle=' "${HISTORY}" || true)"
	passed="$(grep -c 'cycle=PASS' "${HISTORY}" || true)"
	printf '\n%s of %s cycles passed\n' "${passed}" "${total}"
}

main() {
	case "${1:-}" in
	arm)
		if [ "${2:-}" = "--reboot" ]; then
			cmd_arm yes
		else
			cmd_arm no
		fi
		;;
	verify) cmd_verify ;;
	status) cmd_status ;;
	disarm) cmd_disarm ;;
	cycle) cmd_cycle ;;
	archive) cmd_archive ;;
	schedule) cmd_schedule "${2:-}" ;;
	unschedule) cmd_unschedule ;;
	history) cmd_history ;;
	selftest) cmd_selftest ;;
	*)
		cat >&2 <<-USAGE
			usage: ${0##*/} <command>

			  arm [--reboot]  record a baseline and install the boot hook
			  verify          compare against the baseline (run by the hook)
			  status          show the last report
			  disarm          remove the boot hook

			  cycle           record any missed cycle, then arm and reboot
			  schedule <expr> run \`cycle\` on a cron expression
			  unschedule      stop running it
			  history         every archived cycle, and the pass rate
			  archive         fold the latest report into the history now.
			                  \`verify\` already does this, so it is only
			                  needed for a report left by an older copy of
			                  this script. Safe to repeat.
			  selftest        check the two crontab entries do not clobber
			                  each other (saves and restores your crontab),
			                  and that the history records each cycle once

			The verify stage refuses to report a pass unless the kernel boot
			ID has changed, so a failed reboot cannot masquerade as a
			successful restart test.

			\`schedule\` is what makes the check unattended, and it needs a
			machine where rebooting is harmless. Do not schedule it on a
			box anyone is working on.
		USAGE
		return 2
		;;
	esac
}

main "$@"
