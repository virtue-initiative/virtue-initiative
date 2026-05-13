import "./SegmentedControl.css";

type Segment = { label: string; value: string };
type SegmentedControlProps = {
  segments: Segment[];
  value: string;
  onChange: (value: string) => void;
  class?: string;
};

export function SegmentedControl({
  segments,
  value,
  onChange,
  class: className,
}: SegmentedControlProps) {
  return (
    <div class={["vi-segmented-control", className].filter(Boolean).join(" ")}>
      {segments.map((seg) => (
        <button
          key={seg.value}
          class={[
            "vi-segmented-control__item",
            seg.value === value && "is-active",
          ]
            .filter(Boolean)
            .join(" ")}
          onClick={() => onChange(seg.value)}
          type="button"
        >
          {seg.label}
        </button>
      ))}
    </div>
  );
}
