export function Button({ children, onClick, className, icon }: { children: preact.ComponentChildren; onClick: () => void; className?: string; icon?: preact.VNode }) {
  if (icon) {
    return (
      <button class={`btn text-icon ${className}`} onClick={onClick} type="button">
        <div class="icon">{icon}</div>
        <div class="text">{children}</div>
      </button>
    );
  } else {
    return (
      <button class={`btn ${className}`} onClick={onClick} type="button">
        {children}
      </button>
    );
  }
}
