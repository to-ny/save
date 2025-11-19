#!/usr/bin/env bash
set -e

cd /opt/save

if docker images | grep -q 'save-api.*latest'; then
  docker tag save-api:latest save-api:backup || true
fi

docker-compose down || true
docker load < save-api.tar.gz
rm save-api.tar.gz
docker-compose up -d
sleep 5

for i in {1..30}; do
  if curl -sf http://localhost:9000/health >/dev/null; then
    exit 0
  fi
  sleep 2
done

docker-compose logs --tail=50 save-api

if docker images | grep -q 'save-api.*backup'; then
  docker-compose down
  docker tag save-api:backup save-api:latest
  docker-compose up -d
fi

exit 1
