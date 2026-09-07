// SPDX-License-Identifier: AGPL-3.0-or-later
// What a page does with a slot state while it waits for its own open.

/**
 * 'wait' while the slot indexes the session `id`, 'done' when it holds `id` ready or failed, and
 * 'replaced' when a later open put another session in the slot.
 */
export function openOutcome(state, id) {
  if (state.id !== id) {
    return 'replaced';
  }
  return state.state === 'indexing' ? 'wait' : 'done';
}
