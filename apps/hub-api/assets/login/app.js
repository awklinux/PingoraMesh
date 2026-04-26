document.addEventListener("DOMContentLoaded", () => {
  $("login-form").addEventListener("submit", handleLogin);
  checkExistingSession();
});

function $(id) {
  return document.getElementById(id);
}

async function checkExistingSession() {
  try {
    const response = await fetch("/api/admin/auth/me", {
      method: "GET",
      headers: {
        Accept: "application/json",
      },
    });
    if (response.ok) {
      window.location.replace("/");
    }
  } catch (_error) {
    // Ignore and stay on login page.
  }
}

async function handleLogin(event) {
  event.preventDefault();
  hideError();

  const button = $("login-button");
  button.disabled = true;
  button.textContent = "登录中...";

  try {
    const response = await fetch("/api/admin/auth/login", {
      method: "POST",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        username: $("username").value.trim(),
        password: $("password").value,
      }),
    });

    const payload = await response.json().catch(() => null);
    if (!response.ok) {
      throw new Error(
        payload?.error?.message ||
          payload?.message ||
          `登录失败：${response.status} ${response.statusText}`,
      );
    }

    window.location.replace("/");
  } catch (error) {
    showError(error.message || "登录失败，请稍后重试");
    button.disabled = false;
    button.textContent = "登录控制台";
  }
}

function showError(message) {
  const box = $("login-error");
  box.hidden = false;
  box.textContent = message;
}

function hideError() {
  const box = $("login-error");
  box.hidden = true;
  box.textContent = "";
}
