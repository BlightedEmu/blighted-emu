// SPDX-License-Identifier: AGPL-3.0-or-later
// What the live page says about one player, bot or mob: its kind, its state and the rows of the
// Selection box.

/** Mob state names, in `MobState` order. */
export const MOB_STATES = ['asleep', 'idle', 'chase', 'attack', 'return', 'dead'];
/** Kinds of entity on the live map. */
export const KINDS = Object.freeze({ PLAYER: 'player', BOT: 'bot', MOB: 'mob' });

const KIND_NAMES = { [KINDS.PLAYER]: 'Player', [KINDS.BOT]: 'Bot', [KINDS.MOB]: 'Mob' };
const POSITION_DECIMALS = 1;

/** A player's state word: dead, frozen (in a handoff) or alive. */
export function playerState(player) {
  if (player.dead) {
    return 'dead';
  }
  return player.frozen ? 'frozen' : 'alive';
}

/** A player view as an entity: kind (player or bot) and state word added. */
export function playerEntity(player) {
  return { ...player, kind: player.bot ? KINDS.BOT : KINDS.PLAYER, state: playerState(player) };
}

/** A mob view as an entity: kind and state name added. */
export function mobEntity(mob) {
  return { ...mob, kind: KINDS.MOB, state: MOB_STATES[mob.state] ?? String(mob.state) };
}

function position(...metres) {
  return metres.map((value) => value.toFixed(POSITION_DECIMALS)).join(', ');
}

/** The [label, value] rows of the Selection box for an entity of playerEntity or mobEntity. */
export function selectionRows(entity) {
  if (entity.kind === KINDS.MOB) {
    return [
      ['Kind', KIND_NAMES[KINDS.MOB]],
      ['Entity', String(entity.entity)],
      ['Sim', entity.sim],
      ['Position', position(entity.x, entity.y)],
      ['State', entity.state],
      ['Health', `${entity.health} / ${entity.max_health}`],
    ];
  }
  return [
    ['Name', entity.name],
    ['Kind', KIND_NAMES[entity.kind]],
    ['Entity', String(entity.entity)],
    ['Sim', entity.sim],
    ['Position', position(entity.x, entity.y, entity.z)],
    ['State', entity.state],
  ];
}
