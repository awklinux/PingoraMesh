#!/usr/bin/env bash

set -euo pipefail

REMOTE_HOST="${PINGORAHUB_REMOTE_HOST:-root@10.20.3.53}"
REMOTE_PORT="${PINGORAHUB_REMOTE_PORT:-22}"
REMOTE_APP_DIR="${PINGORAHUB_REMOTE_APP_DIR:-/opt/pingorahub}"
REMOTE_SRC_DIR="${PINGORAHUB_REMOTE_SRC_DIR:-$REMOTE_APP_DIR/src/PingoraMesh}"
REMOTE_BIN_DIR="${PINGORAHUB_REMOTE_BIN_DIR:-$REMOTE_APP_DIR/bin}"
REMOTE_ETC_DIR="${PINGORAHUB_REMOTE_ETC_DIR:-/etc/pingorahub}"
REMOTE_SYSTEMD_DIR="${PINGORAHUB_REMOTE_SYSTEMD_DIR:-/etc/systemd/system}"
REMOTE_MIGRATIONS_DIR="${PINGORAHUB_REMOTE_MIGRATIONS_DIR:-$REMOTE_APP_DIR/migrations}"
REMOTE_BUILD_JOBS="${PINGORAHUB_REMOTE_BUILD_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"

PACKAGES=(
  pingorahub-hub-api
  pingorahub-dns-worker
  pingorahub-node-agent
  pingorahub-status-worker
  pingorahub-failover-worker
)

SERVICES=(
  pingorahub-hub-api
  pingorahub-dns-worker
  pingorahub-node-agent
  pingorahub-status-worker
  pingorahub-failover-worker
)

SSH_OPTS=(
  -p "$REMOTE_PORT"
  -o StrictHostKeyChecking=no
)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "[1/6] Sync source to $REMOTE_HOST:$REMOTE_SRC_DIR"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "
  rm -rf '$REMOTE_SRC_DIR'
  mkdir -p '$REMOTE_SRC_DIR'
"
COPYFILE_DISABLE=1 tar -C "$ROOT_DIR" \
  --exclude='.git' \
  --exclude='target' \
  --exclude='.codex-artifacts' \
  -czf - . \
  | ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "tar -xzf - -C '$REMOTE_SRC_DIR'"

echo "[2/6] Prepare remote directories"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "
  id pingorahub >/dev/null 2>&1 || useradd --system --home '$REMOTE_APP_DIR' --shell /usr/sbin/nologin pingorahub
  install -d -o pingorahub -g pingorahub '$REMOTE_APP_DIR'
  install -d -o pingorahub -g pingorahub '$REMOTE_BIN_DIR'
  install -d -o pingorahub -g pingorahub '$REMOTE_SRC_DIR'
  install -d -o pingorahub -g pingorahub '$REMOTE_MIGRATIONS_DIR'
  install -d -o pingorahub -g pingorahub /var/lib/pingorahub
  install -d -o pingorahub -g pingorahub /var/log/pingorahub
  install -d -o pingorahub -g pingorahub '$REMOTE_ETC_DIR'
  chown -R pingorahub:pingorahub '$REMOTE_SRC_DIR'
"

echo "[3/6] Build release binaries on remote host"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "
  set -euo pipefail
  cd '$REMOTE_SRC_DIR'
  cargo build --release --locked -j '$REMOTE_BUILD_JOBS' \
    -p pingorahub-hub-api \
    -p pingorahub-dns-worker \
    -p pingorahub-node-agent \
    -p pingorahub-status-worker \
    -p pingorahub-failover-worker
"

echo "[4/6] Install binaries, migrations and systemd units"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "
  set -euo pipefail
  install -m 755 '$REMOTE_SRC_DIR/target/release/pingorahub-hub-api' '$REMOTE_BIN_DIR/pingorahub-hub-api'
  install -m 755 '$REMOTE_SRC_DIR/target/release/pingorahub-dns-worker' '$REMOTE_BIN_DIR/pingorahub-dns-worker'
  install -m 755 '$REMOTE_SRC_DIR/target/release/pingorahub-node-agent' '$REMOTE_BIN_DIR/pingorahub-node-agent'
  install -m 755 '$REMOTE_SRC_DIR/target/release/pingorahub-status-worker' '$REMOTE_BIN_DIR/pingorahub-status-worker'
  install -m 755 '$REMOTE_SRC_DIR/target/release/pingorahub-failover-worker' '$REMOTE_BIN_DIR/pingorahub-failover-worker'
  cp '$REMOTE_SRC_DIR'/migrations/*.sql '$REMOTE_MIGRATIONS_DIR/'
  install -m 644 '$REMOTE_SRC_DIR/deploy/systemd/pingorahub-hub-api.service' '$REMOTE_SYSTEMD_DIR/pingorahub-hub-api.service'
  install -m 644 '$REMOTE_SRC_DIR/deploy/systemd/pingorahub-dns-worker.service' '$REMOTE_SYSTEMD_DIR/pingorahub-dns-worker.service'
  install -m 644 '$REMOTE_SRC_DIR/deploy/systemd/pingorahub-node-agent.service' '$REMOTE_SYSTEMD_DIR/pingorahub-node-agent.service'
  install -m 644 '$REMOTE_SRC_DIR/deploy/systemd/pingorahub-status-worker.service' '$REMOTE_SYSTEMD_DIR/pingorahub-status-worker.service'
  install -m 644 '$REMOTE_SRC_DIR/deploy/systemd/pingorahub-failover-worker.service' '$REMOTE_SYSTEMD_DIR/pingorahub-failover-worker.service'
  chown -R pingorahub:pingorahub '$REMOTE_BIN_DIR' '$REMOTE_MIGRATIONS_DIR'
"

echo "[5/6] Apply required runtime migration"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" '
  set -euo pipefail
  POSTGRES_URL="$(grep "^PINGORAHUB_POSTGRES_URL=" '"$REMOTE_ETC_DIR"'/hub-api.env | cut -d= -f2-)"
  if [ -n "$POSTGRES_URL" ]; then
    psql "$POSTGRES_URL" -v ON_ERROR_STOP=1 <<SQL
CREATE TABLE IF NOT EXISTS schema_migrations (
  filename TEXT PRIMARY KEY,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
SQL

    legacy_migration_is_applied() {
      migration="$1"
      case "$migration" in
        0001_init.sql)
          psql "$POSTGRES_URL" -Atqc "SELECT 1 FROM information_schema.tables WHERE table_schema = '\''public'\'' AND table_name = '\''nodes'\'' LIMIT 1;"
          ;;
        0002_admin_auth.sql)
          psql "$POSTGRES_URL" -Atqc "SELECT 1 FROM information_schema.tables WHERE table_schema = '\''public'\'' AND table_name = '\''admin_users'\'' LIMIT 1;"
          ;;
        0003_runtime_fixes.sql)
          has_hostname="$(psql "$POSTGRES_URL" -Atqc "SELECT 1 FROM information_schema.columns WHERE table_schema = '\''public'\'' AND table_name = '\''nodes'\'' AND column_name = '\''hostname'\'' LIMIT 1;")"
          has_noop="$(psql "$POSTGRES_URL" -Atqc "SELECT 1 FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid WHERE t.typname = '\''dns_provider_type'\'' AND e.enumlabel = '\''noop'\'' LIMIT 1;")"
          if [ "$has_hostname" = "1" ] && [ "$has_noop" = "1" ]; then
            echo 1
          fi
          ;;
        *)
          ;;
      esac
    }

    find "'"$REMOTE_MIGRATIONS_DIR"'" -maxdepth 1 -type f -name "*.sql" | sort | while read -r migration; do
      filename="$(basename "$migration")"
      applied="$(psql "$POSTGRES_URL" -Atqc "SELECT 1 FROM schema_migrations WHERE filename = '\''$filename'\'' LIMIT 1;")"
      if [ "$applied" = "1" ]; then
        echo "Skipping already applied migration: $filename"
        continue
      fi

      legacy_applied="$(legacy_migration_is_applied "$filename")"
      if [ "$legacy_applied" = "1" ]; then
        echo "Recording legacy-applied migration: $filename"
        psql "$POSTGRES_URL" -v ON_ERROR_STOP=1 -c "INSERT INTO schema_migrations(filename) VALUES ('\''$filename'\'') ON CONFLICT DO NOTHING;"
        continue
      fi

      echo "Applying migration: $filename"
      psql "$POSTGRES_URL" -v ON_ERROR_STOP=1 -f "$migration"
      psql "$POSTGRES_URL" -v ON_ERROR_STOP=1 -c "INSERT INTO schema_migrations(filename) VALUES ('\''$filename'\'') ON CONFLICT DO NOTHING;"
    done
  fi
'

echo "[6/6] Reload and restart services"
ssh "${SSH_OPTS[@]}" "$REMOTE_HOST" "
  set -euo pipefail
  systemctl daemon-reload
  restarted_services=''
  for service in ${SERVICES[*]}; do
    env_name=\"\${service#pingorahub-}.env\"
    env_file=\"$REMOTE_ETC_DIR/\${env_name}\"
    if [ ! -f \"\$env_file\" ]; then
      echo \"Skipping \$service because \$env_file is missing\"
      continue
    fi
    systemctl restart \"\$service\"
    restarted_services=\"\$restarted_services \$service\"
  done
  if [ -n \"\${restarted_services// /}\" ]; then
    systemctl --no-pager --full status \$restarted_services | sed -n '1,240p'
  fi
"

echo "Remote deploy finished: http://10.20.3.53:3000/healthz"
