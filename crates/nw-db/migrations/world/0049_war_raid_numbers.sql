-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Observed (war data 1739, 173 wars and 127 invasions of 320 ledgers): a war's defender side
-- number is its attacker number + 1 and an invasion takes one number; one counter per world.
-- So a war takes the next sequence value too, and the next war starts after its defender.
CREATE FUNCTION territory_war_defender_number() RETURNS trigger AS $$
BEGIN
    IF NEW.attacker_company IS NOT NULL THEN
        PERFORM nextval(pg_get_serial_sequence('territory_war', 'number'));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER territory_war_defender_number_trigger AFTER INSERT ON territory_war
    FOR EACH ROW EXECUTE FUNCTION territory_war_defender_number();
