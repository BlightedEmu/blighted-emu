// SPDX-License-Identifier: AGPL-3.0-or-later
// The login form of a server-mode admin site.

const LOGIN_URL = '/api/auth/login';
const HOME_URL = '/';
const ADMIN_HEADERS = Object.freeze({ 'X-Blighted-Admin': '1', 'Content-Type': 'application/json' });

const form = document.getElementById('login');
const status = document.getElementById('status');

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  status.textContent = '';
  const body = JSON.stringify({
    username: document.getElementById('username').value,
    password: document.getElementById('password').value,
  });
  try {
    const response = await fetch(LOGIN_URL, { method: 'POST', headers: ADMIN_HEADERS, body });
    if (response.ok) {
      window.location.assign(HOME_URL);
      return;
    }
    status.textContent = (await response.text()) || `Login failed (${response.status}).`;
  } catch (error) {
    status.textContent = `Login failed: ${error.message ?? error}`;
  }
});
