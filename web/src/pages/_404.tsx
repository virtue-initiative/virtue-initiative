import "./404.css";

export function NotFound() {
  return (
    <div class="not-found">
      <h1 class="not-found-title">Page not found</h1>
      <p class="not-found-message">
        Sorry, we couldn't find the page you were looking for. It may have been
        moved or deleted.
      </p>
      <div class="not-found-actions">
        <a href="/" class="btn btn-primary">
          Go to dashboard
        </a>
        <a href="/settings" class="btn btn-ghost">
          Settings
        </a>
      </div>
    </div>
  );
}
