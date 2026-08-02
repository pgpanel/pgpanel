-- Persist decryptable agent token so the join link stays available in the UI.
ALTER TABLE panel_remote_access ADD COLUMN agent_token_encrypted TEXT;
