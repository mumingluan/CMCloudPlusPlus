import { useId, useLayoutEffect, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';

export function FileMenuDialog({
  title, target, closeLabel, onClose, children,
}: {
  title: string;
  target?: string;
  closeLabel: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const outsidePress = useRef(false);
  const heading = useId();
  useLayoutEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  return (
    <dialog
      ref={ref}
      className="file-menu-overlay"
      aria-labelledby={heading}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onPointerDown={(event) => {
        outsidePress.current = event.target === event.currentTarget;
      }}
      onPointerCancel={() => { outsidePress.current = false; }}
      onClick={(event) => {
        event.stopPropagation();
        if (outsidePress.current && event.target === event.currentTarget) onClose();
        outsidePress.current = false;
      }}
    >
      <div className="file-menu-panel">
        <header className="file-menu-header">
          <div>
            <strong id={heading}>{title}</strong>
            {target && <span title={target}>{target}</span>}
          </div>
          <button type="button" className="icon-button file-menu-close" aria-label={closeLabel} onClick={onClose}>
            <X size={20} />
          </button>
        </header>
        {children}
      </div>
    </dialog>
  );
}
