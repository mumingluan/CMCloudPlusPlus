import { useLayoutEffect, useRef, useState } from 'react';
import { transferTabs } from './transferView';
import type { TransferFilter } from './transferView';

export function TransferCategories({
  value,
  onChange,
}: {
  value: TransferFilter;
  onChange: (value: TransferFilter) => void;
}) {
  const container = useRef<HTMLDivElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const [dropdown, setDropdown] = useState(false);
  useLayoutEffect(() => {
    const measure = () => {
      const available = container.current?.clientWidth ?? 0;
      if (available > 0) setDropdown((tabs.current?.scrollWidth ?? 0) > available);
    };
    measure();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', measure);
      return () => window.removeEventListener('resize', measure);
    }
    const observer = new ResizeObserver(measure);
    if (container.current) observer.observe(container.current);
    if (tabs.current) observer.observe(tabs.current);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={container} className={`transfer-categories ${dropdown ? 'is-dropdown' : ''}`}>
      <div ref={tabs} className="transfer-tab-groups" aria-hidden={dropdown || undefined}>
        {transferTabs.map((group, i) => (
          <div className="tabs" key={i}>
            {group.map(([filter, text]) => (
              <button
                key={filter}
                className={value === filter ? 'selected' : ''}
                aria-pressed={value === filter}
                tabIndex={dropdown ? -1 : undefined}
                onClick={() => onChange(filter)}
              >
                {text}
              </button>
            ))}
          </div>
        ))}
      </div>
      {dropdown && (
        <select
          className="transfer-category-select"
          aria-label="传输分类"
          value={value}
          onChange={(event) => onChange(event.target.value as TransferFilter)}
        >
          {transferTabs.map((group, i) => (
            <optgroup key={i} label={['上传', '下载', '全部'][i]}>
              {group.map(([filter, text]) => (
                <option key={filter} value={filter}>
                  {text}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      )}
    </div>
  );
}
