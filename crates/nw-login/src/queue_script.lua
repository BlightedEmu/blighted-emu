-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Login queue update for one world, atomic in Valkey (M2 spec section 8).
-- KEYS[1] order (zset ticket -> join ms), KEYS[2] refresh (zset ticket -> last refresh ms),
-- KEYS[3] admitted (zset ticket -> expiry ms), KEYS[4] free player slots (from world-director).
-- ARGV: 1 now ms (empty: use the Valkey clock), 2 stale-after ms, 3 capacity, 4 entry seconds, 5 world id,
-- 6 entry hash prefix, 7 record prefix, 8 account pointer prefix (per world),
-- 9 action ("join" or "refresh"), 10 account id, 11 character id, 12 new ticket id, 13 ticket id.
local now = tonumber(ARGV[1])
if not now then
  -- One clock for every edge-http copy: Valkey's own time.
  local time = redis.call('TIME')
  now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)
end
local stale_before = now - tonumber(ARGV[2])
local entry_prefix, record_prefix, account_prefix = ARGV[6], ARGV[7], ARGV[8]

local function forget(ticket)
  redis.call('ZREM', KEYS[1], ticket)
  redis.call('ZREM', KEYS[2], ticket)
  redis.call('ZREM', KEYS[3], ticket)
  redis.call('DEL', entry_prefix .. ticket, record_prefix .. ticket)
end

-- Remove waiting entries that stopped refreshing.
for _, ticket in ipairs(redis.call('ZRANGEBYSCORE', KEYS[2], '-inf', '(' .. stale_before)) do
  local account = redis.call('HGET', entry_prefix .. ticket, 'account')
  forget(ticket)
  if account and redis.call('GET', account_prefix .. account) == ticket then
    redis.call('DEL', account_prefix .. account)
  end
end

local ticket = false
if ARGV[9] == 'join' then
  local existing = redis.call('GET', account_prefix .. ARGV[10])
  if existing then
    local same = redis.call('HGET', entry_prefix .. existing, 'character') == ARGV[11]
    local alive = redis.call('ZSCORE', KEYS[1], existing) or redis.call('EXISTS', record_prefix .. existing) == 1
    if same and alive then
      ticket = existing
    else
      forget(existing)
    end
  end
  if not ticket then
    ticket = ARGV[12]
    redis.call('HSET', entry_prefix .. ticket, 'account', ARGV[10], 'character', ARGV[11])
    redis.call('ZADD', KEYS[1], now, ticket)
    redis.call('SET', account_prefix .. ARGV[10], ticket)
  end
else
  ticket = ARGV[13]
  if redis.call('HGET', entry_prefix .. ticket, 'account') ~= ARGV[10] then
    return {'unknown'}
  end
end
if redis.call('ZSCORE', KEYS[1], ticket) then
  redis.call('ZADD', KEYS[2], now, ticket)
end

-- Admit from the front while the entry ticket limit and the free player slots allow it. No
-- report from the director means no free slot.
redis.call('ZREMRANGEBYSCORE', KEYS[3], '-inf', now)
local live = redis.call('ZCARD', KEYS[3])
local capacity = math.min(tonumber(ARGV[3]), tonumber(redis.call('GET', KEYS[4])) or 0)
local entry_seconds = tonumber(ARGV[4])
while live < capacity do
  local head = redis.call('ZRANGE', KEYS[1], 0, 0)
  if #head == 0 then
    break
  end
  local admitted = head[1]
  local account = redis.call('HGET', entry_prefix .. admitted, 'account')
  local character = redis.call('HGET', entry_prefix .. admitted, 'character')
  local expires_ms = now + entry_seconds * 1000
  local record = cjson.encode({
    ticket_id = admitted,
    account_id = account,
    world_id = ARGV[5],
    character_id = character,
    expires_at = math.floor(expires_ms / 1000),
  })
  redis.call('SET', record_prefix .. admitted, record, 'EX', entry_seconds)
  redis.call('ZADD', KEYS[3], expires_ms, admitted)
  redis.call('ZREM', KEYS[1], admitted)
  redis.call('ZREM', KEYS[2], admitted)
  redis.call('EXPIRE', entry_prefix .. admitted, entry_seconds)
  redis.call('EXPIRE', account_prefix .. account, entry_seconds)
  live = live + 1
end

local record = redis.call('GET', record_prefix .. ticket)
if record then
  return {'admitted', ticket, record}
end
local rank = redis.call('ZRANK', KEYS[1], ticket)
if rank then
  return {'waiting', ticket, tostring(rank + 1)}
end
return {'unknown'}
