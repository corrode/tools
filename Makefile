# Detect platform for shared library extension
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    SPELLFIX_LIB := ext/spellfix.dylib
    SQLITE_INCLUDE := $(shell brew --prefix sqlite)/include
else
    SPELLFIX_LIB := ext/spellfix.so
    SQLITE_INCLUDE := /usr/include
endif

SERVER_USER := root
SERVER_IP := 46.225.7.147
REMOTE_DIR := /data/coolify/applications/search/data
STATIC_REMOTE_DIR := $(REMOTE_DIR)/static
TIMESTAMP := $(shell date +%Y%m%d_%H%M%S)

.PHONY: help ext
help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z0-9_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

ext: $(SPELLFIX_LIB) ## Compile the spellfix1 SQLite extension for local development

$(SPELLFIX_LIB): ext/spellfix.c
	cc -fPIC -shared -o $@ $< -I$(SQLITE_INCLUDE)
	@echo "Built $@"

.PHONY: lint
lint: ## Run format check and clippy
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings 
	
.PHONY: fix
fix: ## Run format and clippy with auto-fix 
	cargo fmt --all 
	cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings 

.PHONY: test
test: ## Run all tests
	cargo test --workspace

.PHONY: format fmt
format fmt: ## Format the code
	cargo fmt --all

.PHONY: dev
dev: $(SPELLFIX_LIB) ## Run the server in watch mode
	cargo watch -x 'run --bin server'

.PHONY: docs
docs: ## Open documentation 
	cargo doc --document-private-items --workspace --open 

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

.PHONY: static-sync
static-sync: ## Bidirectional sync of thumbnails between local and remote
	@echo "Syncing thumbnails (bidirectional)..."
	mkdir -p data/static/youtube
	ssh $(SERVER_USER)@$(SERVER_IP) "mkdir -p $(STATIC_REMOTE_DIR)/youtube"
	@echo "Pulling from remote..."
	rsync -avz --progress $(SERVER_USER)@$(SERVER_IP):$(STATIC_REMOTE_DIR)/youtube/ data/static/youtube/
	@echo "Pushing to remote..."
	rsync -avz --progress data/static/youtube/ $(SERVER_USER)@$(SERVER_IP):$(STATIC_REMOTE_DIR)/youtube/

.PHONY: deploy
deploy: db-copy static-sync ## Deploy database and sync static files with remote
	@echo "Deployment complete"
