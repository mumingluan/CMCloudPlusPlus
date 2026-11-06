import { useLayoutEffect, useRef, useState } from 'react';
import type { LucideIcon } from 'lucide-react';

export type FileMenuItem = {
  label: string;
  icon: LucideIcon;
  action: () => void;
  danger?: boolean;
};

export function FileContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: FileMenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const menu = ref.current!;
    const previous = document.activeElement as HTMLElement | null;
    const bounds = menu.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8)),
    });
    menu.querySelector<HTMLButtonElement>('button')?.focus();
    const outside = (event: PointerEvent) => {
      if (!menu.contains(event.target as Node)) onClose();
    };
    const dismiss = () => onClose();
    const scroll = (event: Event) => {
      if (!menu.contains(event.target as Node)) onClose();
    };
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('blur', dismiss);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('blur', dismiss);
      if (menu.contains(document.activeElement)) previous?.focus();
    };
  }, [x, y, onClose]);
  return (
    <div
      ref={ref}
      className="file-context-menu"
      role="menu"
      aria-label="文件操作"
      style={position}
      onKeyDown={(event) => {
        const buttons = [...ref.current!.querySelectorAll<HTMLButtonElement>('button')];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
          event.preventDefault();
          const next =
            event.key === 'Home'
              ? 0
              : event.key === 'End'
                ? buttons.length - 1
                : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
          buttons[next]?.focus();
        } else if (event.key === 'Escape' || event.key === 'Tab') {
          event.preventDefault();
          onClose();
        }
      }}
    >
      {items.map(({ label, icon: Icon, action, danger }) => (
        <button
          key={label}
          role="menuitem"
          className={danger ? 'danger' : ''}
          onClick={() => {
            onClose();
            action();
          }}
        >
          <Icon size={16} />
          <span>{label}</span>
        </button>
      ))}
    </div>
  );
}
