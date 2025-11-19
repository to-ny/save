#!/usr/bin/env bash
set -e

SERVER_IP="${1:?Server IP required}"

echo "Waiting for SSH on ${SERVER_IP}..."
for i in {1..60}; do
  if ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 "root@${SERVER_IP}" "echo ready" 2>/dev/null; then
    echo "SSH ready"
    break
  fi
  if [ $i -eq 60 ]; then
    echo "SSH timeout after 5 minutes"
    exit 1
  fi
  sleep 5
done

echo "Waiting for cloud-init..."
for i in {1..60}; do
  if ssh "root@${SERVER_IP}" "test -f /var/lib/cloud/instance/boot-finished" 2>/dev/null; then
    echo "cloud-init complete"
    break
  fi
  if [ $i -eq 60 ]; then
    echo "cloud-init timeout after 5 minutes"
    exit 1
  fi
  sleep 5
done

echo "Waiting for Docker..."
for i in {1..120}; do
  if ssh "root@${SERVER_IP}" "command -v docker >/dev/null 2>&1 && systemctl is-active docker >/dev/null 2>&1" 2>/dev/null; then
    echo "Docker ready"
    break
  fi
  if [ $i -eq 120 ]; then
    echo "Docker timeout after 10 minutes"
    exit 1
  fi
  sleep 5
done

echo "Waiting for docker-compose..."
for i in {1..60}; do
  if ssh "root@${SERVER_IP}" "command -v docker-compose >/dev/null 2>&1" 2>/dev/null; then
    echo "docker-compose ready"
    break
  fi
  if [ $i -eq 60 ]; then
    echo "docker-compose timeout after 5 minutes"
    exit 1
  fi
  sleep 5
done

ssh "root@${SERVER_IP}" "mkdir -p /opt/save"
