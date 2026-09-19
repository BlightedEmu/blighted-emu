// SPDX-License-Identifier: AGPL-3.0-or-later
// The to-do page's task changes, with no page access (admin studio spec section 10).

/** The state filter that keeps every task. */
export const ALL_STATES = 'all';

/** The tasks in `state` (or every task for {@link ALL_STATES}). */
export function tasksIn(tasks, state) {
  return state === ALL_STATES ? tasks : tasks.filter((task) => task.state === state);
}

/** `task` with the design file `path` linked; a blank or linked path changes nothing. */
export function withRecord(task, path) {
  const record = path.trim();
  if (record === '' || task.records.includes(record)) {
    return task;
  }
  return { ...task, records: [...task.records, record] };
}

/** `task` without the design file `path`. */
export function withoutRecord(task, path) {
  return { ...task, records: task.records.filter((record) => record !== path) };
}

/** The body of a task change: every field but the id. */
export function changeBody(task) {
  return {
    title: task.title,
    text: task.text ?? '',
    kind: task.kind,
    state: task.state,
    taken_by: task.taken_by ?? null,
    issue: task.issue ?? null,
    records: task.records,
  };
}
