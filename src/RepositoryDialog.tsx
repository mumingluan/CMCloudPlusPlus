import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';

export function RepositoryDialog({
  titleId,
  busy,
  onDismiss,
  children,
  returnFocus,
  onReturnFocus,
  focusKey = 'initial',
  compact = false,
}: {
  titleId: string;
  busy: boolean;
  onDismiss: () => void;
  children: ReactNode;
  returnFocus?: HTMLElement | null;
  onReturnFocus?: (target: HTMLElement | null) => void;
  focusKey?: string;
  compact?: boolean;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const dismiss = useRef(onDismiss);
  dismiss.current = onDismiss;
  const blocked = useRef(busy);
  const focused = useRef<string | null>(null);
  blocked.current = busy;
  useEffect(() => {
    const element = dialog.current!;
    const previous = returnFocus ?? document.activeElement;
    element.showModal();
    return () => {
      element.close();
      if (onReturnFocus) onReturnFocus(previous instanceof HTMLElement ? previous : null);
      else if (
        previous instanceof HTMLElement &&
        previous.isConnected &&
        !previous.matches(':disabled')
      )
        previous.focus();
    };
  }, []);
  useEffect(() => {
    if (busy || focused.current === focusKey) return;
    const first = dialog.current?.querySelector<HTMLElement>('[data-dialog-autofocus]:not(:disabled)')
      ?? dialog.current?.querySelector<HTMLElement>(
      'input:not(:disabled), button:not(:disabled), select:not(:disabled), textarea:not(:disabled)',
    );
    if (first) {
      first.focus();
      focused.current = focusKey;
    }
  }, [busy, focusKey]);
  return createPortal(
    <dialog
      className={`repository-modal${compact ? ' repository-modal-compact' : ''}`}
      ref={dialog}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        if (!blocked.current) dismiss.current();
      }}
    >
      {children}
    </dialog>,
    document.body,
  );
}
