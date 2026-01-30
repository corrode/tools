SERVER_USER := root
SERVER_IP := 46.225.7.147
REMOTE_DIR := /data/coolify/applications/search/data
TIMESTAMP := $(shell date +%Y%m%d_%H%M%S)

.PHONY: help
help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z0-9_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

.PHONY: lint
lint: ## Run format check and clippy
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings 

.PHONY: format fmt
format fmt: ## Format the code
	cargo fmt --all

.PHONY: dev
dev: ## Run the server in watch mode
	cargo watch -x 'run --bin server'

.PHONY: docker
docker: ## Build the Docker image
	docker build -t search .

.PHONY: db-copy
db-copy: ## Copy local DB to remote server
	ssh $(SERVER_USER)@$(SERVER_IP) "mkdir -p $(REMOTE_DIR)"
	@echo "Deploying data/index.db to $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db"
	scp data/index.db $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db

.PHONY: db-backup
db-backup: ## Backup remote DB to local backups/
	mkdir -p backups
	@echo "Fetching $(REMOTE_DIR)/index.db to backups/index_$(TIMESTAMP).db"
	scp $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db backups/index_$(TIMESTAMP).db