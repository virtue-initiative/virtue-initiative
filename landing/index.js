function toggleTheme() {
  var next =
    document.documentElement.getAttribute("data-theme") === "dark"
      ? "light"
      : "dark";
  document.documentElement.setAttribute("data-theme", next);
  localStorage.setItem("theme", next);
  document.getElementById("theme-toggle").textContent =
    next === "dark" ? "☀" : "🌙";
}

document.getElementById("theme-toggle").addEventListener("click", toggleTheme);

document.getElementById("theme-toggle").textContent =
  document.documentElement.getAttribute("data-theme") === "dark"
    ? "☀"
    : "🌙";

