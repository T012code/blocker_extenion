#!/usr/bin/env bash
# Tests the Smart Clip migration on a throwaway local PostgreSQL (14 or newer).
# Run from anywhere: supabase/tests/run-tests.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="$(ls -d /usr/lib/postgresql/*/bin 2>/dev/null | sort -V | tail -1)"
[ -n "$bin" ] || bin="$(dirname "$(command -v initdb)")"
tmp="$(mktemp -d)"
cleanup() { "$bin/pg_ctl" -D "$tmp/db" -m immediate stop >/dev/null 2>&1 || true; rm -rf "$tmp"; }
trap cleanup EXIT
run() { if [ "$(id -u)" = 0 ]; then chown -R postgres "$tmp"; su postgres -c "$*"; else bash -c "$*"; fi; }
mkdir -p "$tmp/db"
run "'$bin/initdb' -D '$tmp/db' -A trust -U postgres >/dev/null"
run "'$bin/pg_ctl' -D '$tmp/db' -o '-k $tmp -p 54329 -c listen_addresses=' -l '$tmp/log' start -w >/dev/null"
psql=("$bin/psql" -h "$tmp" -p 54329 -U postgres -d postgres -X -v ON_ERROR_STOP=1 -q)
"${psql[@]}" -f "$here/stubs.sql"
# pg_cron isn't part of plain PostgreSQL: the stubs stand in for it.
sed 's/^create extension if not exists pg_cron;/-- (pg_cron: stubbed for the tests)/' "$here/../migrations/"*.sql | "${psql[@]}"
"${psql[@]}" -f "$here/tests.sql" 2>&1 | sed 's/^psql:[^:]*:[0-9]*: NOTICE:  /  /; s/^NOTICE:  /  /'
