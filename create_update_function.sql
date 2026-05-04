
CREATE FUNCTION update_modified_at_column() RETURNS TRIGGER AS $$ BEGIN NEW.modified_at = now();
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trigger_graphs_modified_at BEFORE
UPDATE ON graphs FOR EACH ROW EXECUTE FUNCTION update_modified_at_column();