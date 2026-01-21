#!/bin/sh
set -eu

# Configuration
COMPOSE_DIR="${COMPOSE_DIR:-/opt/ndld}"
HEALTH_URL="${HEALTH_URL:-http://localhost:8080/health}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-30}"

die() {
    echo "ERROR: $1"
    exit 1
}

# Validate COMPOSE_DIR
case "$COMPOSE_DIR" in
    /*) ;; # absolute path, ok
    *)  die "COMPOSE_DIR must be an absolute path: $COMPOSE_DIR" ;;
esac

[ -d "$COMPOSE_DIR" ] || die "COMPOSE_DIR does not exist: $COMPOSE_DIR"
[ -f "$COMPOSE_DIR/docker-compose.yml" ] || [ -f "$COMPOSE_DIR/compose.yml" ] || \
    die "No compose file found in $COMPOSE_DIR"

cd "$COMPOSE_DIR"

# Capture current image IDs for rollback
PREV_IMAGES=$(docker compose images -q 2>/dev/null || true)

echo "Pulling latest images..."
if ! docker compose pull; then
    die "Pull failed"
fi

# Check if images actually changed
NEW_IMAGES=$(docker compose images -q 2>/dev/null || true)
if [ "$PREV_IMAGES" = "$NEW_IMAGES" ]; then
    echo "No new images, skipping restart"
    exit 0
fi

echo "New images detected, restarting services..."
if ! docker compose up -d; then
    echo "Restart failed, attempting rollback..."
    docker compose down 2>/dev/null || true
    docker compose up -d 2>/dev/null || die "Rollback failed"
    die "Restart failed (rollback attempted)"
fi

# Health check
echo "Waiting for health check..."
elapsed=0
while [ $elapsed -lt "$HEALTH_TIMEOUT" ]; do
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        echo "Health check passed"
        echo "Done"
        exit 0
    fi
    sleep 2
    elapsed=$((elapsed + 2))
done

echo "WARNING: Health check failed after ${HEALTH_TIMEOUT}s (service may still be starting)"
exit 0
