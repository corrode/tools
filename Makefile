SERVER_USER := root
SERVER_IP := 46.225.7.147
REMOTE_DIR := /data/coolify/applications/search/data

.PHONY: deploy-db
deploy-db:
	ssh $(SERVER_USER)@$(SERVER_IP) "mkdir -p $(REMOTE_DIR)"
	@echo "Deploying data/index.db to $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db"
	scp data/index.db $(SERVER_USER)@$(SERVER_IP):$(REMOTE_DIR)/index.db