import './Tabs.css';

type Tab = { label: string; value: string };
type TabsProps = {
  tabs: Tab[];
  value: string;
  onChange: (value: string) => void;
  class?: string;
};

export function Tabs({ tabs, value, onChange, class: className }: TabsProps) {
  return (
    <div class={['vi-tabs', className].filter(Boolean).join(' ')} role="tablist">
      {tabs.map((tab) => (
        <button
          key={tab.value}
          role="tab"
          class={['vi-tab', tab.value === value && 'vi-tab--active'].filter(Boolean).join(' ')}
          aria-selected={tab.value === value}
          onClick={() => onChange(tab.value)}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
