SCRIPTS_DIR := scripts
TERRAFORM_DIR := terraform

.PHONY: help check deploy-smoke deploy-medium deploy-large teardown status logs logs-follow cleanup cleanup-delete diagnose test-local

.DEFAULT_GOAL := help

help:
	@echo "Save Load Test Infrastructure"
	@echo ""
	@echo "Deployment:"
	@echo "  make deploy-smoke    Deploy smoke test (2 vCPU, 2GB)"
	@echo "  make deploy-medium   Deploy medium (4 vCPU, 8GB)"
	@echo "  make deploy-large    Deploy large (8 vCPU, 16GB)"
	@echo "  make teardown        Destroy infrastructure"
	@echo ""
	@echo "Testing:"
	@echo "  make test-local      Run load tests on server (localhost, no WAN)"
	@echo ""
	@echo "Monitoring:"
	@echo "  make status          Show server status"
	@echo "  make logs            Show recent logs"
	@echo "  make logs-follow     Follow logs in real-time"
	@echo "  make diagnose        Run diagnostics"
	@echo ""
	@echo "Utilities:"
	@echo "  make check           Check prerequisites"
	@echo "  make cleanup         List orphaned resources"
	@echo "  make cleanup-delete  Delete orphaned resources"
	@echo ""
	@echo "Setup: cp .env.template .env && edit .env && source .env"

check:
	@$(SCRIPTS_DIR)/check-prereqs.sh

deploy-smoke:
	@$(SCRIPTS_DIR)/deploy.sh smoke

deploy-medium:
	@$(SCRIPTS_DIR)/deploy.sh medium

deploy-large:
	@$(SCRIPTS_DIR)/deploy.sh large

teardown:
	@$(SCRIPTS_DIR)/teardown.sh

status:
	@cd $(TERRAFORM_DIR) && terraform output 2>/dev/null || echo "No infrastructure deployed"

logs:
	@$(SCRIPTS_DIR)/logs.sh

logs-follow:
	@$(SCRIPTS_DIR)/logs.sh --follow

cleanup:
	@$(SCRIPTS_DIR)/cleanup-hcloud.sh

cleanup-delete:
	@$(SCRIPTS_DIR)/cleanup-hcloud.sh --delete

diagnose:
	@$(SCRIPTS_DIR)/diagnose.sh

test-local:
	@$(SCRIPTS_DIR)/ssh-test.sh
