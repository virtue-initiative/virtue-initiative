import "./Avatar.css";

type AvatarSize = "sm" | "md" | "lg";
type AvatarProps = {
  src?: string;
  name?: string;
  size?: AvatarSize;
  class?: string;
};

export function Avatar({ src, name, size = "md", class: className }: AvatarProps) {
  const initials = name
    ? name
        .split(" ")
        .map((w) => w[0])
        .join("")
        .slice(0, 2)
        .toUpperCase()
    : "?";
  return (
    <span
      class={["vi-avatar", `vi-avatar--${size}`, className]
        .filter(Boolean)
        .join(" ")}
    >
      {src ? (
        <img src={src} alt={name ?? ""} class="vi-avatar__img" />
      ) : (
        <span class="vi-avatar__initials">{initials}</span>
      )}
    </span>
  );
}
