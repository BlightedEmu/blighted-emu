// SPDX-License-Identifier: AGPL-3.0-or-later
// The page theme: night watch (dark) unless the viewer chose the day theme. A plain script in
// <head>, so the page never shows the other theme first. The choice stays in this browser.
(() => {
  const DARK = 'dark';
  const LIGHT = 'light';
  const STORAGE_KEY = 'blighted-admin-theme';
  const BUTTON_ID = 'theme';
  const root = document.documentElement;

  function saved() {
    try {
      return localStorage.getItem(STORAGE_KEY);
    } catch {
      return null;
    }
  }

  function keep(theme) {
    try {
      localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      // Without storage the choice lasts until the page is left.
    }
  }

  root.dataset.theme = saved() === LIGHT ? LIGHT : DARK;
  document.addEventListener('DOMContentLoaded', () => {
    const button = document.getElementById(BUTTON_ID);
    const label = () => {
      button.textContent = root.dataset.theme === DARK ? 'Day' : 'Night';
      button.title = root.dataset.theme === DARK ? 'Use the day (light) theme' : 'Use the night (dark) theme';
    };
    label();
    button.addEventListener('click', () => {
      root.dataset.theme = root.dataset.theme === DARK ? LIGHT : DARK;
      keep(root.dataset.theme);
      label();
    });
  });
})();
