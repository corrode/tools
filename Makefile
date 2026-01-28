SERVER_USER := root
SERVER_IP := 46.225.7.147
REMOTE_DIR := /data/coolify/applications/search/data
TIMESTAMP := $(shell date +%Y%m%d_%H%M%S)

.PHONY: db-copy
db-copy:
	ssh $(SERVER_USER)@$(SERVER_IP) "mkdir -p $(REMOTE_DIR)"
	@echo "Deploying data/index.db to $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db"
	scp data/index.db $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db

.PHONY: db-backup
db-backup:
	mkdir -p backups
	@echo "Fetching $(REMOTE_DIR)/index.db to backups/index_$(TIMESTAMP).db"
	scp $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db backups/index_$(TIMESTAMP).db