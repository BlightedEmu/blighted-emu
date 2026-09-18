// SPDX-License-Identifier: AGPL-3.0-or-later
// The Apply line of the studio page (bar + word + detail), and the rows of design problems.

/** The Apply steps in order (the server's `ApplyStep`). */
export const APPLY_STEPS = Object.freeze(['check', 'write', 'restart', 'wait_sims']);

const STEP_WORDS = Object.freeze({
  check: 'Checking',
  write: 'Writing',
  restart: 'Restarting',
  wait_sims: 'Waiting for sims',
});
const PHASE_WORDS = Object.freeze({ idle: 'Idle', done: 'Done', failed: 'Failed' });
const FULL_BAR = 1;
const EMPTY_BAR = 0;

function stepShare(step) {
  const index = APPLY_STEPS.indexOf(step);
  return index < 0 ? EMPTY_BAR : index / APPLY_STEPS.length;
}

/** The word, the detail line and the bar share (0 to 1) of an Apply state. */
export function applyLine(status) {
  const detail = status.detail ?? '';
  if (status.phase === 'running') {
    return { word: STEP_WORDS[status.step], detail, bar: stepShare(status.step) };
  }
  const bar = status.phase === 'done' ? FULL_BAR : status.phase === 'failed' ? stepShare(status.step) : EMPTY_BAR;
  return { word: PHASE_WORDS[status.phase], detail, bar };
}

function problemText(problem) {
  const { code, detail } = problem;
  if (detail === undefined || detail === null) {
    return code;
  }
  const text = typeof detail === 'object'
    ? Object.entries(detail).map(([key, value]) => `${key} ${value}`).join(', ')
    : String(detail);
  return `${code}: ${text}`;
}

/** Design problems as table rows, sorted by file, then field. */
export function errorRows(errors) {
  return errors
    .map((error) => ({ file: error.path, field: error.field ?? '', problem: problemText(error.problem) }))
    .sort((left, right) => left.file.localeCompare(right.file) || left.field.localeCompare(right.field));
}
