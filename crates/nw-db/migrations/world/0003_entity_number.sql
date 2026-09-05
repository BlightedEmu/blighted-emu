-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Entity numbers of the world (M5 spec section 10.1). The director takes one per join. The top
-- is 2^35 - 1: a bundle member holds the number in a 5-byte prefix code.

CREATE SEQUENCE entity_number AS BIGINT MINVALUE 1 MAXVALUE 34359738367 NO CYCLE;
